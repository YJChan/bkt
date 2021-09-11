use circle_rs::{Infinite, Progress};
use dirs::home_dir;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use quicli::prelude::*;
use s3::creds::Credentials;
use s3::{Bucket, Region};
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind, Read, Write};
use std::path::Path;
use std::time::Instant;
use structopt::StructOpt;
use tokio::fs::create_dir_all;
use walkdir::WalkDir;

#[derive(Debug, Deserialize)]
struct Config {
    access_key: String,
    secret_key: String,
    bucket: String,
    endpoint: String,
    region: String,
}

#[derive(Debug, StructOpt)]
struct Cli {
    #[structopt(
        help = "allowed arguments are <get> or <put> or <rm> or <set> or <list-config> or <count>"
    )]
    action: String,

    #[structopt(long, short, help = "File you want to upload to s3 bucket")]
    source: Option<String>,

    #[structopt(long, short, help = "Location you want to put in s3 bucket")]
    destination: Option<String>,

    #[structopt(long, short, help = "Recursively upload files in folder to s3 bucket")]
    folder: Option<String>,

    // Quick and easy logging setup you get for free with quicli
    #[structopt(flatten)]
    verbosity: Verbosity,

    #[structopt(
        long = "config",
        short,
        number_of_values = 5,
        name = "config-value",
        help = "s3 bucket configuration, sequence as follow:\n access key (s3 access key)\n secret key (s3 secret key)\n bucket (use '-' on bucket if no fix bucket need to define) \n endpoint (use '-' on endpoint if no endpoint need to configure, default is AWS endpoint) \n region (use '-' on region if that is s3 compatible services)"
    )]
    config: Vec<String>,

    #[structopt(
        long,
        short,
        name = "bucket-name",
        help = "Bucket name, this value will overwrite the bucket name set in config"
    )]
    bucket: Option<String>,

    #[structopt(
        long = "content-type",
        short = "t",
        help = "Upload file's content type, eg. image/jpeg, application/pdf, etc..."
    )]
    content_type: Option<String>,

    #[structopt(
        long,
        short,
        help = "Limit number of files to be upload when uploading a folder"
    )]
    limit: Option<String>,

    #[structopt(long, short, help = "Put files with N workers")]
    worker: Option<String>,
}

async fn setup_config(
    access_key: &str,
    secret_key: &str,
    bucket: &str,
    endpoint: &str,
    region: &str,
) -> Result<(), &'static str> {
    let config = format!(
        r#"access_key = "{}"
secret_key = "{}"
bucket = "{}"
endpoint = "{}"
region = "{}""#,
        access_key, secret_key, bucket, endpoint, region
    );
    let config_dir = home_dir().unwrap().join(".bkt");
    create_dir_all(&config_dir).await.unwrap();

    let path = Path::new(&config_dir).join("config.toml");
    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", path.display(), why),
        Ok(file) => file,
    };

    match file.write_all(config.as_bytes()) {
        Err(why) => {
            panic!("couldn't write to {}: {}", path.display(), why)
        }
        Ok(_) => {
            println!("Successfully setup configuration. To update config, please run the same command with different arguments.");
            Ok(())
        }
    }
}

fn read_config() -> Result<Config, Error> {
    let config_path = home_dir().unwrap().join(".bkt").join("config.toml");

    let mut file = match File::open(&config_path) {
        Err(_) => {
            return Err(Error::new(ErrorKind::NotFound, "config file is not set, please run \nbkt set --config <access-key> <secret-key>, <bucket>, <endpoint>, <region>"));
        }
        Ok(file) => file,
    };

    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();

    let config: Config = toml::from_str(&s).unwrap();
    Ok(config)
}

async fn init_bucket(bucket_name: Option<String>) -> Result<Bucket, Error> {
    let mut config = match read_config() {
        Err(err) => {
            return Err(err);
        }
        Ok(config) => config,
    };
    if let Some(bkt) = bucket_name {
        config.bucket = bkt.into();
    };
    let region: Region = if config.endpoint != "-" && config.region == "-" {
        Region::Custom {
            region: "".into(),
            endpoint: config.endpoint,
        }
    } else if config.endpoint == "-" && config.region != "-" {
        config.region.parse().unwrap()
    } else {
        return Err(Error::new(
            ErrorKind::Other,
            "s3 region is not correctly configured",
        ));
    };

    let credentials = Credentials::new(
        Some(&config.access_key),
        Some(&config.secret_key),
        None,
        None,
        None,
    )
    .unwrap();

    let s3_bucket = Bucket::new(&config.bucket, region, credentials).unwrap();

    Ok(s3_bucket)
}

async fn push_object(
    src: &str,
    dest: &str,
    alt_bucket_name: Option<String>,
    content_type: Option<String>,
) -> Result<u16, Error> {
    let s3_bucket = init_bucket(alt_bucket_name).await.unwrap();

    match File::open(src) {
        Err(why) => {
            return Err(why);
        }
        Ok(file) => {
            let mut bytes = Vec::new();
            let mut reader = BufReader::new(file);
            reader.read_to_end(&mut bytes).unwrap();
            if let Some(content_type) = content_type {
                let result = s3_bucket
                    .put_object_with_content_type(dest, &bytes.to_vec(), &content_type)
                    .await
                    .unwrap();
                Ok(result.1)
            } else {
                let result = s3_bucket.put_object(dest, &bytes.to_vec()).await.unwrap();
                Ok(result.1)
            }
        }
    }
}

async fn push_objects(
    src: &str,
    dest: &str,
    alt_bucket_name: Option<String>,
) -> Result<(i32, i32, i32), Error> {
    if Path::new(src).exists() && Path::new(src).is_dir() {
        // let mut loader = Infinite::new().to_stderr();
        // loader.set_msg("Uploading");
        // let _start_thread = loader.start()?;
        let now = Instant::now();

        let mut fail_count: i32 = 0;
        let mut success_count: i32 = 0;

        let total_count: u64 = WalkDir::new(src)
            .contents_first(true)
            .into_iter()
            .filter_map(|dir| {
                if dir.unwrap().path().is_file() {
                    Some(1)
                } else {
                    None
                }
            })
            .sum();

        let pb = ProgressBar::new(total_count);
        for file in WalkDir::new(src).contents_first(true).into_iter() {
            match file {
                Err(err) => {
                    fail_count = fail_count + 1;
                    println!("{}", err.into_io_error().unwrap());
                }
                Ok(dir_entry) => {
                    if dir_entry.path().is_file() {
                        // println!("is file ? {}", dir_entry.path().is_file());
                        // println!("file: {}", dir_entry.path().display());
                        let src_file = format!("{}", dir_entry.path().display());
                        let s3_file_dest = dir_entry.path().to_string_lossy().replace(src, dest);
                        // println!("Pushing to {}", s3_file_dest);
                        push_object(&src_file, &s3_file_dest, alt_bucket_name.clone(), None)
                            .await
                            .unwrap();
                        // std::thread::sleep(std::time::Duration::from_millis(1000));
                        success_count = success_count + 1;
                    }
                }
            };
            pb.inc(1);
        }

        // loader.stop()?;
        pb.finish_with_message("done");
        println!("Finished in {:?}", now.elapsed());
        Ok((fail_count, success_count, fail_count + success_count))
    } else {
        Err(Error::new(
            ErrorKind::NotFound,
            "Folder not found with provided path",
        ))
    }
}

#[tokio::main]
async fn main() -> CliResult {
    let args = Cli::from_args();
    let styles = [
        ("Uploading: ", "█  ", "red"),
        ("Uploading: ", "█▉▊▋▌▍▎▏  ", "yellow"),
        ("Uploading: ", "█▇▆▅▄▃▂▁  ", "green"),
        ("Uploading: ", "█▓▒░  ", "blue"),
        ("Uploading: ", "█▛▌▖  ", "magenta"),
    ];

    let action = args.action.to_lowercase();

    match &*action {
        "get" => {
            println!("Function not implemented");
        }
        "put" => {
            if let (Some(src), Some(dest)) = (args.source, &args.destination) {
                let mut loader = Infinite::new().to_stderr();
                loader.set_msg("Uploading");
                let _start_thread = loader.start()?;
                let now = Instant::now();

                match push_object(&src, &dest, args.bucket, args.content_type).await {
                    Ok(code) => {
                        loader.stop()?;
                        println!("Finished in {:?}", now.elapsed());
                        println!("File successfully put with status code: {}", code)
                    }
                    Err(err) => {
                        loader.stop()?;
                        println!("Put file error for {} : {}", src, err)
                    }
                };
            } else if let (Some(folder), Some(dest)) = (args.folder, &args.destination) {
                if let Some(w) = args.worker {
                    let worker_size: i16 = w.parse().unwrap();
                    let cpus = num_cpus::get();
                    println!("Machine has {} cpu(s)", cpus);
                    println!("Running with {} worker thread(s)", w);                    
                    
                    if (worker_size as usize) / cpus > 100 {
                        println!("WARNING");
                        println!("You might be running too many threads...");
                    }

                    let src_folder_list: Vec<String> = WalkDir::new(&folder)
                        .contents_first(true)
                        .into_iter()
                        .filter_map(|dir| {
                            let directory = dir.unwrap();
                            if directory.path().is_file() {
                                Some(directory.path().to_str().unwrap().to_string())
                            } else {
                                None
                            }
                        })
                        .collect();
                    
                    println!("Uploading {} file(s)", src_folder_list.len());
                    println!("-------------------------------------------------");
                    
                    let m = MultiProgress::new();
                    let chunk_size = src_folder_list.len() / worker_size as usize;
                    let chunks_folder_list: Vec<Vec<String>> = src_folder_list
                        .chunks(chunk_size)
                        .map(|c| c.to_owned())
                        .collect();

                    let bucket = if let Some(bucket) = args.bucket {
                        bucket.to_string()
                    } else {
                        String::new()
                    };

                    let handlers: Vec<_> = chunks_folder_list
                        .into_iter()
                        .map(|folder_list| {
                            let pb = m.add(ProgressBar::new(folder_list.len() as u64));
                            pb.set_style(
                                ProgressStyle::default_bar()
                                    .template(&format!(
                                        "{{prefix:.bold}}▕{{bar:.{}}}▏{{msg}}",
                                        styles[1].2
                                    ))
                                    .progress_chars(styles[1].1),
                            );
                            let tmp_bkt_name = bucket.clone();
                            let destination = dest.clone();
                            tokio::spawn(async move {
                                let b = if tmp_bkt_name.is_empty() {
                                    None
                                } else {
                                    Some(tmp_bkt_name)
                                };
                                let now = Instant::now();
                                let len = folder_list.len();
                                let mut index = 0;

                                for file in folder_list {
                                    match push_object(&file, &destination, b.clone(), None).await {
                                        Ok(_) => (),
                                        Err(err) => {
                                            println!("Put file error for {} : {}", file, err);
                                        }
                                    };
                                    index = index + 1;
                                    pb.set_prefix(format!("[{}/{}] {:.6}s", index, len, now.elapsed().as_secs_f32()));
                                    pb.inc(1);
                                    pb.set_message(format!("{:3}%", 100 * index / len));
                                    // thread::sleep(std::time::Duration::from_millis(1000));
                                }
                                pb.finish_with_message("100%");
                            })
                        })
                        .collect();

                    for h in handlers {
                        let _ = h.await.unwrap();
                    }
                } else {
                    match push_objects(&folder, &dest, args.bucket).await {
                        Ok(count) => {
                            println!("{} failed to upload", count.0);
                            println!("{} successed to upload", count.1);
                            println!("{} total number of files processed", count.2);
                        }
                        Err(err) => println!("Put folder error for {} : {}", folder, err),
                    }
                }
            }
        }
        "list-config" => {
            match read_config() {
                Err(err) => {
                    println!("{}", err);
                }
                Ok(config) => {
                    println!("Current bkt configuration");
                    println!("-------------------------");
                    println!("access-key    : {}", config.access_key);
                    println!("secret-key    : {}", config.secret_key);
                    println!("bucket        : {}", config.bucket);
                    println!("endpoint      : {}", config.endpoint);
                    println!("region        : {}", config.region);
                }
            };
        }
        "set" => {
            if !&args.config.is_empty() {
                let access_key = &args.config[0];
                let secret_key = &args.config[1];
                let bucket = &args.config[2];
                let endpoint = &args.config[3];
                let region = &args.config[4];

                setup_config(access_key, secret_key, bucket, endpoint, region)
                    .await
                    .unwrap();
            } else {
                println!("current <set> action only supported for config, please run \nbkt set --config <access-key> <secret-key>, <bucket>, <endpoint>, <region>")
            }
        }
        "rm" => {
            println!("Function not implemented");
        }
        "count" => {
            let src = if let Some(src) = args.folder {
                src
            } else {
                std::env::current_dir().unwrap().display().to_string()
            };

            let total_count: u64 = WalkDir::new(&src)
                .contents_first(true)
                .into_iter()
                .filter_map(|dir| {
                    if dir.unwrap().path().is_file() {
                        Some(1)
                    } else {
                        None
                    }
                })
                .sum();
            println!("Number of files in {}: {}", src, total_count);
        }
        _ => println!("Invalid action, only <get> or <put> is allowed."),
    };

    Ok(())
}
