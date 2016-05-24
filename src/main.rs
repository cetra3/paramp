extern crate zip;
extern crate yaml_rust;

use yaml_rust::{yaml,YamlLoader};
use zip::read::{ZipArchive, ZipFile};
use std::fs::{self, File};
use std::path::Path;
use std::io::{copy, Write, Read, BufReader, BufRead, Error, ErrorKind, Result};
use std::collections::{HashMap, HashSet};
use std::env;

fn main() {

    let args: Vec<String> = env::args().collect();

    match args.len() {
        2 => generate_entirely_from_yaml(&args[1]),
        3 => generate_from_yaml_with_output(&args[1], &args[2]),
        _ => {
            println!("PARAMP: Generate an Alfresco deployment with modules");
            println!("Usage: {} source.yaml <target_dir>", args[0])
        }
    }
}

fn generate_entirely_from_yaml(input_file: &str) {

    match read_file(resolve_file(input_file).unwrap()) {
        Ok(contents) => {
            let input = YamlLoader::load_from_str(&contents).unwrap();

            match &input[0]["output_dir"] {
                &yaml::Yaml::String(ref output_dir) => {

                    generate_from_yaml_with_output(input_file, output_dir);

                }
                _ => { println!("Could not find an output_dir property in input yaml file, specify an output directory with `output_dir` on the command line or within the yaml file");}
            }

        },
        _ => {}
    }


}

fn generate_from_yaml_with_output(input_file: &str, output_dir: &str) {

    match fs::remove_dir_all(output_dir) {
        Ok(_) => {
            println!("   Clearing dir: {}", output_dir);
        },
        _ => {}
    }

    match read_file(resolve_file(input_file).unwrap()) {
        Ok(contents) => {
            let input = YamlLoader::load_from_str(&contents).unwrap();

            match &input[0]["files"] {
                &yaml::Yaml::Array(ref v) => {
                    for amp in v {

                        let amp = amp.as_str().unwrap();

                        println!("Extracting file: {}", amp);

                        generate_output(amp, output_dir);
                    }
                },
                _ => {}
            }

        },
        _ => {}
    }

}

fn generate_output(input_file: &str, output_dir: &str) {

    let file = File::open(input_file).unwrap();

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
                              "the file cannot be found"))
    }
}
