extern crate zip;
extern crate yaml_rust;
extern crate clap;
extern crate crypto;

#[macro_use]
extern crate hyper;

use yaml_rust::{Yaml,YamlLoader};
use zip::read::{ZipArchive, ZipFile};
use std::fs::{self, File};
use std::path::Path;
use std::io::{copy, Write, Read, BufReader, BufRead, Error, ErrorKind, Result};
use std::collections::{HashMap, HashSet};

use crypto::md5::Md5;
use crypto::digest::Digest;
use std::fmt;

use hyper::Client;
use hyper::status::StatusCode;

use clap::{Arg, App};

header! { (Token, "TOKEN") => [String] }

struct AmpModule {
    vendor: String,

    name: String,

    version: String,

    module_type: String

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
        write!(f, "{}:{}:{}:{}", self.vendor, self.name, self.version, self.module_type)
    }
}

fn main() {

    let matches = App::new("Paramp")
        .version("1.0.0")
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
        .get_matches();

    let input_file = matches.value_of("yaml_file").unwrap();

    let yaml = get_yaml(input_file);

    let output_dir = matches.value_of("output_dir")
        .map(|dir| String::from(dir))
        .unwrap_or_else(|| get_yaml_string(&yaml, "output_dir").expect("Could not get target directory from YAML file"));

    let token: Option<String> = matches.value_of("token")
        .map(|token| String::from(token))
        .or(get_yaml_string(&yaml, "token"));

    let module_type: Option<String> = matches.value_of("type")
        .map(|token| String::from(token))
        .or(get_yaml_string(&yaml, "type"));


    let mut files = Vec::new();

    if module_type.is_some() && token.is_some() {

        let url: String = matches.value_of("url")
            .map(|token| String::from(token))
            .or(get_yaml_string(&yaml, "url"))
            .unwrap_or(String::from("https://repo.parashift.com.au"));

        let modules = get_yaml_string_list(&yaml, "modules");

        files.append(&mut download_files(&modules, &module_type.unwrap(), &token.unwrap(), &url));

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

fn download_files(modules: &Vec<String>, module_type: &str, token: &str, url: &str) -> Vec<String> {

    fs::create_dir_all(".ampcache").unwrap();

    let mut return_files = Vec::new();

    let client = Client::new();

    for module in modules.into_iter().map(|module| AmpModule::new(module, module_type)) {

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

                    return_files.push(file_name);
                } else {
                    panic!("Could not get '{}' (Invalid Server Checksum)", module)
                }

            },
            StatusCode::SeeOther => {
                println!("Skipping module '{}' (No '{}' component)", module, module_type)
            }
            status => panic!("Could not get '{}' ({})", module, status)
        }
    }

    return return_files

}

fn compare_checksum(mut file: File, checksum: String) -> bool {

    let mut sh = Md5::new();

    let mut buf = Vec::new();

    file.read_to_end(&mut buf).unwrap();

    sh.input(&buf);

    let file_sum = sh.result_str();

    return file_sum == checksum;

}

fn get_yaml_string_list(yaml: &Yaml, value: &str) -> Vec<String> {
    match yaml[value] {
        Yaml::Array(ref array) => {
            array.into_iter().map(|value| String::from(value.as_str().unwrap())).collect()
        }
        _ => Vec::new()
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

fn create_file_and_dirs(file: &str) -> Result<File> {
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

fn read_file(mut file: File) -> Result<String> {
    let mut s = String::new();
    match file.read_to_string(&mut s) {
        Ok(_) => Ok(s),
        Err(_) => Err(Error::new(ErrorKind::InvalidInput,
                      "the file cannot be read"))
    }
}

fn resolve_file(search_path: &str) -> Result<File> {
    let path = Path::new(search_path);
    match path.exists(){
        true => File::open(&path),
        false =>
                Err(Error::new(ErrorKind::NotFound,
                              format!("the file at {} cannot be found", search_path)))
    }
}
