extern crate zip;
extern crate yaml_rust;
extern crate clap;
extern crate crypto;
extern crate rustc_serialize;
extern crate semver;
extern crate regex;
extern crate toml;
extern crate memmap;

#[macro_use]
extern crate hyper;

#[macro_use]
extern crate lazy_static;

use std::env;
use yaml_rust::{Yaml,YamlLoader};
use zip::read::{ZipArchive, ZipFile};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::io::{self,copy, Write, Read, BufReader, BufRead, Error, ErrorKind};
use std::collections::{HashMap, HashSet};

use crypto::md5::Md5;
use crypto::digest::Digest;
use std::fmt;

use hyper::Client;
use hyper::status::StatusCode;

use clap::{Arg, App};

use rustc_serialize::{json, Decodable, Decoder};
use semver::Version;

use regex::Regex;

use memmap::{Mmap, Protection};

header! { (Token, "TOKEN") => [String] }

lazy_static! {

    /*
        Let's try deal with Alfresco's weird numbering!
    */

    static ref MAJOR_MINOR_PATCH_MINI: Regex = {
        Regex::new(r"(?P<major>\d)\.(?P<minor>\d)\.(?P<patch>\d).(?P<mini>\d)").unwrap()
    };

    static ref MAJOR_MINOR_PRE: Regex = {
        Regex::new(r"(?P<major>\d)\.(?P<minor>\d)-(?P<pre>.*)").unwrap()
    };

    static ref MAJOR_MINOR: Regex = {
        Regex::new(r"(?P<major>\d)\.(?P<minor>\d)").unwrap()
    };

}

#[derive(Debug, Clone, RustcDecodable, RustcEncodable, PartialEq, Eq, PartialOrd, Ord)]
struct AmpModule {
    vendor: String,

    name: String,

    version: String,

    module_type: String

}

#[derive (Debug, Clone, RustcDecodable)]
struct Config {
    url: String,
    token: String,
    matchers: Vec<AmpMatcher>
}

#[derive (Debug, Clone)]
struct AmpMatcher {
    vendor: String,
    name: String,
    regex: Regex,
}

impl Decodable for AmpMatcher {
    fn decode<D: Decoder>(d: &mut D) -> Result<AmpMatcher, D::Error> {

        d.read_struct("AmpMatcher", 3, |d| {
            let vendor = try!(d.read_struct_field("vendor", 0, |d| { d.read_str() }));
            let name = try!(d.read_struct_field("name", 0, |d| { d.read_str() }));
            let regex_raw = try!(d.read_struct_field("regex", 0, |d| { d.read_str() }));
            let regex = Regex::new(&regex_raw).unwrap();

            Ok(AmpMatcher{
                vendor: vendor,
                name: name,
                regex: regex
            })
        })
    }
}


#[derive(Debug)]
struct VersionPair {
    original: String,
    version: Version
}

impl AmpModule{
    fn new(module: &str, module_type:&str) -> AmpModule {
        let parts: Vec<&str> = module.split(":").collect();

        AmpModule{
            vendor: String::from(parts[0]),
            name: String::from(parts[1]),
            version: String::from(parts[2]),
            module_type: String::from(module_type)
        }

    }
}

impl fmt::Display for AmpModule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.module_type != "" {
            write!(f, "{}:{}:{}:{}", self.vendor, self.name, self.version, self.module_type)
        } else {
            write!(f, "{}:{}:{}", self.vendor, self.name, self.version)
        }


    }
}

fn main() {


    let matches = App::new("Paramp")
        .version("1.1.1")
        .author("Peter Lesty <peter@parashift.com.au>")
        .about("Generate an Alfresco deployment with modules")
        .arg(Arg::with_name("yaml_file")
            .help("Source Yaml file for modules")
            .required(true)
            .index(1))
        .arg(Arg::with_name("output_dir")
            .help("Target Directory")
            .index(2))
        .arg(Arg::with_name("token")
            .help("Override Config Token")
            .short("t")
            .takes_value(true))
        .arg(Arg::with_name("type")
            .help("Module Type, either 'repo' or 'share'")
            .short("m")
            .takes_value(true))
        .arg(Arg::with_name("url")
            .help("URL of Packages server")
            .short("u")
            .takes_value(true))
        .arg(Arg::with_name("check")
            .help("Check for latest versions")
            .short("c"))
        .arg(Arg::with_name("refresh")
            .help("When Checking: Refresh older formatted yaml files")
            .short("r"))
        .arg(Arg::with_name("dev")
            .help("When Checking: Include Non-QA Passed modules")
            .short("d"))
        .get_matches();

    let input_file = matches.value_of("yaml_file").unwrap();

    let yaml = get_yaml(input_file);

    let token: Option<String> = matches.value_of("token")
        .map(|token| String::from(token))
        .or(get_yaml_string(&yaml, "token"));

    let module_type: Option<String> = matches.value_of("type")
        .map(|token| String::from(token))
        .or(get_yaml_string(&yaml, "type"));

    if matches.is_present("check") {

        let mut url: String = matches.value_of("url")
            .map(|token| String::from(token))
            .or(get_yaml_string(&yaml, "url"))
            .unwrap_or(String::from("https://repo.parashift.com.au"));

        let mut modules: Vec<AmpModule> = Vec::new();

        if matches.is_present("refresh") {

            let mut default_config_file = env::home_dir().unwrap_or(PathBuf::new());
            default_config_file.push(".parelease");
            //default to oldschool
            match File::open(default_config_file) {
                Ok(file) => {

                    println!("Refreshing from older format\n");

                    let config_string = read_file(file).unwrap();
                    let config: Config = toml::decode_str(&config_string).unwrap();

                    //Update the URL to the config file
                    url = config.url.clone();

                    modules.append(&mut get_old_versions(&yaml, &config));

                    println!("-\n");

                },
                _ => ()

            }
        }

        modules.append(&mut get_yaml_string_list(&yaml, "alfresco_modules").iter().map(|module| AmpModule::new(&module, "")).collect());
        modules.sort();
        modules.dedup();

        let include_dev: bool = matches.is_present("dev") || get_yaml_bool(&yaml, "development").unwrap_or(false);

        if modules.len() > 0 {
            if include_dev {
                println!("Checking versions (Dev included)\n");
            } else {
                println!("Checking versions\n");
            }


            format_module_list(check_versions(&url, modules, include_dev))
        } else {
            println!("No modules found in yaml file!");

        }

    } else {

        let output_dir = matches.value_of("output_dir")
            .map(|dir| String::from(dir))
            .unwrap_or_else(|| get_yaml_string(&yaml, "output_dir").expect("Could not get target directory from YAML file"));

        let mut files = Vec::new();

        let modules = get_yaml_string_list(&yaml, "alfresco_modules");

        if modules.len() > 0 {
            if module_type.is_some() {

                let url: String = matches.value_of("url")
                    .map(|url| String::from(url))
                    .or(get_yaml_string(&yaml, "url"))
                    .unwrap_or(String::from("https://repo.parashift.com.au"));


                files.append(&mut download_files(&modules, &module_type.unwrap(), &token.unwrap_or(String::from("")), &url));

            } else {
                println!("Skipping module download, no module type is set");
            }
        }


        match fs::remove_dir_all(&output_dir) {
            Ok(_) => {

                println!("Clearing dir: {}", output_dir);
            },
            _ => {}
        }

        files.append(&mut get_yaml_string_list(&yaml, "files"));

        output_files(files, &output_dir);
    }
}

fn resolve_module(file_name: &str, config: &Config) -> Option<AmpModule> {

    for matcher in config.matchers.iter() {
        match matcher.regex.captures(file_name) {
            Some(captured_values) => {

                let version = captured_values.name("version").unwrap();

                return Some(AmpModule {
                    vendor: matcher.vendor.clone(),
                    name: matcher.name.clone(),
                    version: String::from(version),
                    module_type: String::from("")
                })
            },
            None => ()
        }
    }
    None
}

fn get_type_version(yaml: &Yaml, module_type: &str) -> Vec<String> {
    match yaml["modules_for_alfresco"][module_type] {
        Yaml::Array(ref array) => {
            array.into_iter().map(|value| String::from(value.as_str().unwrap())).collect()
        },
        _ => Vec::new()
    }
}

fn get_old_versions(yaml: &Yaml, config: &Config) -> Vec<AmpModule> {

    let mut return_vec: Vec<AmpModule> = Vec::new();

    let mut file_names: Vec<String> = Vec::new();

    file_names.append(&mut get_type_version(yaml, "repo"));
    file_names.append(&mut get_type_version(yaml, "share"));

    for candidate in file_names.iter() {
        match resolve_module(candidate, config) {
            Some(module) => {
                return_vec.push(module);
            },
            None => {
                println!("Could not convert filename '{}' to a known module", candidate);
            }
        }
    }

    return_vec.sort();
    return_vec.dedup();


    return_vec
}

fn format_module_list(modules: Vec<AmpModule>) {

    println!("\nPaste the following into your yaml file:\n\n```");
    println!("alfresco_modules:");

    let mut enterprise_module: Option<AmpModule> = None;

    for module in modules.iter() {
        if module.vendor != "alfresco" || (module.name != "enterprise" && module.name != "community")  {
            println!("  - {}:{}:{}", module.vendor, module.name, module.version);
        } else {
            enterprise_module = Some(module.clone());
        }
    }

    match enterprise_module {
        Some(module) => {
            println!("  - {}:{}:{}", module.vendor, module.name, module.version);
        },
        _ => ()
    }

    println!("```");

}

fn check_versions(url: &str, modules: Vec<AmpModule>, include_dev: bool) -> Vec<AmpModule> {

    let mut return_modules: Vec<AmpModule> = Vec::new();

    let client = Client::new();

    for module in modules.into_iter() {
        let submit_url = match include_dev {
            true => format!("{}/module/{}/{}?dev=true", url, module.vendor, module.name),
            false => format!("{}/module/{}/{}", url, module.vendor, module.name)
        };

        let mut response = client.get(&submit_url)
            .send()
            .unwrap();

        match response.status {
            StatusCode::Ok => {

                let mut response_body = String::new();

                response.read_to_string(&mut response_body).expect("Could not read response");

                let existing_version = get_version(&module.version);

                let version_array: Vec<String> = json::decode(&response_body).unwrap();

                let versions_found = version_array.len() > 0;

                let mut versions: Vec<VersionPair> = version_array.iter()
                    .map(|version| get_version(&version))
                    .filter(|pair| pair.version.gt(&existing_version.version))
                    .collect();

                if versions.len() > 0 {

                    versions.sort_by(| left, right | left.version.cmp(&right.version).reverse());

                    let ref candidate = versions[0];

                    println!("Module '{}' can be upgraded to version '{}'", module, candidate.original);

                    return_modules.push(AmpModule {
                        name: module.name,
                        module_type: module.module_type,
                        version: candidate.original.clone(),
                        vendor: module.vendor
                    });


                } else {
                    if versions_found {
                        return_modules.push(module);
                    } else {
                        println!("Could not find any versions for '{}'", module);
                    }

                }


            },
            status => panic!("Could not get '{}' ({})", module, status)
        }
    }

    return return_modules;

}

fn download_files(modules: &Vec<String>, module_type: &str, token: &str, url: &str) -> Vec<String> {

    fs::create_dir_all(".ampcache").unwrap();

    modules.iter()
        .map(|module| AmpModule::new(module, module_type))
        .map(|module| {

            println!("Checking module:{}", module);

            let client = Client::new();

            let file_name = format!(".ampcache/{}-{}-{}-{}.amp", module.vendor, module.name, module.version, module.module_type);

            let submit_url = format!("{}/module/{}/{}/{}/{}", url, module.vendor, module.name, module.version, module.module_type);

            let mut response = client.get(&submit_url)
                .send()
                .unwrap();

            match response.status {
                StatusCode::Ok => {
                    let mut checksum = String::new();

                    response.read_to_string(&mut checksum).expect("Could not read response");

                    if checksum.len() > 0 {
                        let local_file = resolve_file(&file_name);

                        if !local_file.is_ok() || !compare_checksum(local_file.unwrap(), checksum) {
                            let mut new_file = create_file_and_dirs(&file_name).unwrap();

                            let mut file_dl = client.get(&*format!("{}.amp", submit_url))
                                .header(Token(String::from(token)))
                                .send()
                                .unwrap();

                            match file_dl.status {
                                StatusCode::Ok => {
                                    println!("Downloading '{}'", module);
                                    copy(&mut file_dl, &mut new_file).expect("Error saving file!");
                                },
                                status => panic!("Could not get '{}' ({})", module, status)
                            }
                        }

                        return Some(file_name);
                    } else {
                        panic!("Could not get '{}' (Invalid Server Checksum)", module)
                    }
                },
                StatusCode::SeeOther => {
                    println!("Skipping module '{}' (No '{}' component)", module, module_type);
                    return None;
                }
                status => panic!("Could not get '{}' ({})", module, status)
            }
        })
        .filter(|filename| *filename != None)
        .map(|filename| filename.unwrap())
        .collect::<Vec<String>>()


}

fn compare_checksum(file: File, checksum: String) -> bool {

    let mut sh = Md5::new();

    match Mmap::open(&file, Protection::Read) {
        Ok(input_map) => {

            //Unsafety comes from the fact that if someone modifies the file while it's being read
            let bytes: &[u8] = unsafe { input_map.as_slice() };

            sh.input(&bytes);

            let file_sum = sh.result_str();

            return file_sum == checksum;
        },
        _ => false
    }
}

fn get_version(input: &str) -> VersionPair {

    match Version::parse(input) {
        Ok(version) => VersionPair {
            original: String::from(input),
            version: version
        },
        _ => {

            match MAJOR_MINOR_PATCH_MINI.captures(input) {
                Some(values) => {

                    let doctored_version = format!("{}.{}.{}-{}", values.name("major").unwrap(), values.name("minor").unwrap(), values.name("patch").unwrap(), values.name("mini").unwrap());

                    return VersionPair {
                        original: String::from(input),
                        version: Version::parse(&doctored_version).unwrap()
                    }
                },
                _ => ()
            }

            match MAJOR_MINOR_PRE.captures(input) {
                Some(values) => {

                    let doctored_version = format!("{}.{}.0-{}", values.name("major").unwrap(), values.name("minor").unwrap(), values.name("pre").unwrap());

                    return VersionPair {
                        original: String::from(input),
                        version: Version::parse(&doctored_version).unwrap()
                    }
                },
                _ => ()
            }

            match MAJOR_MINOR.captures(input) {
                Some(values) => {

                    let doctored_version = format!("{}.{}.0", values.name("major").unwrap(), values.name("minor").unwrap());

                    return VersionPair {
                        original: String::from(input),
                        version: Version::parse(&doctored_version).unwrap()
                    }
                },
                _ => {

                    return VersionPair {
                        original: String::from(input),
                        version: Version::parse("0.0.0").unwrap()
                    }
                }
            }
        }

    }

}


fn get_yaml_string_list(yaml: &Yaml, value: &str) -> Vec<String> {
    match yaml[value] {
        Yaml::Array(ref array) => {
            array.into_iter().map(|value| String::from(value.as_str().unwrap())).collect()
        }
        _ => Vec::new()
    }

}

fn get_yaml_bool(yaml:&Yaml, value:&str) -> Option<bool> {
    match yaml[value] {
        Yaml::Boolean(ref yaml_value) => Some(yaml_value.clone()),
        _ => None
    }
}

fn get_yaml_string(yaml: &Yaml, value: &str) -> Option<String> {
    match yaml[value] {
        Yaml::String(ref yaml_value) => Some(yaml_value.clone()),
        _ => None
    }
}

fn get_yaml(input_file: &str) -> Yaml {
    match read_file(resolve_file(input_file).unwrap()) {
        Ok(contents) => YamlLoader::load_from_str(&contents).unwrap()[0].clone(),
        Err(_) => panic!("Could not read file")
    }
}

fn output_files(input_files: Vec<String>, output_dir: &str) {

    for file in input_files.iter() {
        generate_output(&file, output_dir);
    }

}

fn generate_output(input_file: &str, output_dir: &str) {

    println!("Extracting file: {}", input_file);

    let file = resolve_file(input_file).unwrap();

    let mut archive = ZipArchive::new(file).unwrap();

    let mut file_map = get_default_map();

    let exclusion_map = get_exclusion_map();

    match archive.by_name("file-mapping.properties") {
        Ok(amp_map) => {
            file_map = decorate_map(amp_map);
        },
        _ => {}
    }

    match archive.by_name("module.properties") {
        Ok(module_file) => {
            create_module_file(module_file, output_dir);
        },
        _ => {}
    }

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();

        if !exclusion_map.contains(file.name()) {

            let mut change_filename = String::from(format!("/{}",file.name()));

            for (from, to) in &file_map {
                if change_filename.starts_with(from) {
                    change_filename = format!("{}{}", to , &change_filename[from.len()..]);
                }
            }

            let new_file = create_file_and_dirs(&*format!("{}/{}", output_dir, change_filename));

            match new_file {
                Ok(mut file_handle) => {
                    copy(&mut file, &mut file_handle).unwrap();
                    //println!("{}", change_filename);
                },
                _ => {}
            }

        }

    }
}

fn create_module_file(file: ZipFile, output_dir: &str) {

    let reader = BufReader::new(file);

    let mut output_file: Option<String> = None;

    let mut output_content = String::new();

    for line in reader.lines() {
        match line {
            Ok(property) => {

                let components: Vec<&str> = property.split("=").collect();
                if components.len() == 2 {

                    let name = components[0].trim();
                    let value = components[1].trim();

                    output_content.push_str(&*format!("{}={}\n", name, value));

                    if name == "module.id" {
                        output_file = Some(format!("{}/WEB-INF/classes/alfresco/module/{}/module.properties", output_dir, value));
                    }
                }
            },
            _ => {}
        }
    }

    match output_file {
        Some(file_name) => {
            let new_file = create_file_and_dirs(&*file_name);

            match new_file {
                Ok(mut file_handle) => {

                    output_content.push_str("module.installState=INSTALLED\n");

                    file_handle.write(&output_content.into_bytes()).unwrap();
                },
                _ => {}
            }
        },
        _ => {}
    }

}

fn create_file_and_dirs(file: &str) -> io::Result<File> {
    create_parent_dirs(file);
    return File::create(file);
}

fn create_parent_dirs(file: &str) {
    let path = Path::new(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
}

fn decorate_map(amp_map: ZipFile) -> HashMap<String, String> {

    let mut return_map = get_default_map();

    let reader = BufReader::new(amp_map);

    for line in reader.lines() {
        match line {
            Ok(map) => {
                let components: Vec<&str> = map.split("=").collect();
                if components.len() == 2 {
                    return_map.insert(String::from(components[0]), String::from(components[1]));
                }
            },
            _=> {}
        }
    }

    return_map
}

fn get_exclusion_map() -> HashSet<String> {
    let mut return_set: HashSet<String> = HashSet::new();

    return_set.insert(String::from("file-mapping.properties"));
    return_set.insert(String::from("module.properties"));

    return_set
}

fn get_default_map() -> HashMap<String,String> {

    let mut return_map: HashMap<String,String> = HashMap::new();

    return_map.insert(String::from("/config"),String::from("/WEB-INF/classes"));
    return_map.insert(String::from("/lib"),String::from("/WEB-INF/lib"));
    return_map.insert(String::from("/licenses"),String::from("/WEB-INF/licenses"));
    return_map.insert(String::from("/web/jsp"),String::from("/jsp"));
    return_map.insert(String::from("/web/css"),String::from("/css"));
    return_map.insert(String::from("/web/images"),String::from("/images"));
    return_map.insert(String::from("/web/scripts"),String::from("/scripts"));
    return_map.insert(String::from("/web/php"),String::from("/"));

    return_map
}

fn read_file(mut file: File) -> io::Result<String> {
    let mut s = String::new();
    match file.read_to_string(&mut s) {
        Ok(_) => Ok(s),
        Err(_) => Err(Error::new(ErrorKind::InvalidInput,
                      "the file cannot be read"))
    }
}

fn resolve_file(search_path: &str) -> io::Result<File> {
    let path = Path::new(search_path);
    match path.exists(){
        true => File::open(&path),
        false =>
                Err(Error::new(ErrorKind::NotFound,
                              format!("the file at {} cannot be found", search_path)))
    }
}
