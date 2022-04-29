use actix_web::web::Bytes;
use actix_web::web::post;
use actix_web::web::resource;
use actix_web::App;
use actix_web::HttpServer;
use actix_web::HttpRequest;
use actix_web::HttpResponse;
use actix_web::dev::ServiceResponse;

use actix_form_data::{Error, Field, Form, Value};

use actix_files::Files;
use actix_files::Directory;

use urlencoding::encode;

use temp_file::empty;

use storage_list::list_partitions;

use sys_mount::SupportedFilesystems;
use sys_mount::UnmountFlags;
use sys_mount::MountFlags;
use sys_mount::Mount;
use sys_mount::unmount;

use futures_util::stream::Stream;
use futures_util::stream::StreamExt;

use std::path::PathBuf;
use std::time::SystemTime;
use std::pin::Pin;
use std::fs::OpenOptions;
use std::fs::copy;
use std::fs::remove_file;
use std::fs::create_dir;
use std::fs::remove_dir;
use std::fs::remove_dir_all;
use std::fs::read_dir;
use std::fs::rename;
use std::fs::metadata;
use std::path::Path;
use std::io::Write;
use std::io::Error as IoError;

fn fmt_date(date: Result<SystemTime, IoError>) -> String {
	match date {
		Ok(t) => match t.duration_since(SystemTime::UNIX_EPOCH) {
			Ok(d) => format!("{}", d.as_secs()),
			_ => String::from("?"),
		},
		_ => String::from("?"),
	}
}

fn directory_renderer(dir: &Directory, req: &HttpRequest) -> Result<ServiceResponse, IoError> {
	let mut dir_data;
	if req.path() == "/" {
		dir_data = String::from("overview");
		for device in list_partitions()? {
			for partition in device.partitions {
				dir_data += ";";
				dir_data += &encode(&partition.name);
				dir_data += ",";
				dir_data += &encode(&format!("{}", partition.capacity));
				dir_data += ",";
				dir_data += &encode(match Path::new(&partition.name).exists() {
					false => "0",
					true => "1",
				});
				dir_data += ",";
				dir_data += &encode(match partition.read_only {
					false => "0",
					true => "1",
				});
			}
		}
	} else {
		dir_data = String::from("directory");
		for entry in read_dir(&dir.path)? {
			if let Ok(entry) = entry {
				if let Ok(name) = entry.file_name().into_string() {
					dir_data += ";";
					dir_data += &encode(&name);
					dir_data += ",";
					if let Ok(stat) = metadata(entry.path()) {
						dir_data += &encode(&fmt_date(stat.modified()));
						dir_data += ",";
						dir_data += &format!("{}", stat.len());
					} else {
						dir_data += "%3F,%3F";
					}
				}
			}
		}
	}
	let body = include_str!("index.html").replace("REPLACED", &dir_data);
	// let body = std::fs::read_to_string("src/index.html").unwrap().replace("REPLACED", &dir_data);
	let resp = HttpResponse::Ok().body(body);
	Ok(ServiceResponse::new(req.clone(), resp))
}

fn redirect(path: &str) -> HttpResponse {
	let path = format!("/{}", path);
	HttpResponse::Found()
		.append_header(("Location", &path[..]))
		.finish()
}

async fn delete_endpoint(form_data: Value<()>) -> HttpResponse {
	let mut map = form_data.map().expect("no form_data?");
	let path_value = map.remove("path").expect("no path in request?");
	let entry_value = map.remove("entry").expect("no entry in request?");
	let mut path = path_value.text().expect("path not a string?");
	let orig_len = path.len();
	let entry = entry_value.text().expect("entry not a string?");

	path += &entry;
	if let Err(_) = remove_file(&path) {
		remove_dir_all(&path).expect("couldn't remove?");
	}

	redirect(&path[..orig_len])
}

async fn rename_endpoint(form_data: Value<()>) -> HttpResponse {
	let mut map = form_data.map().expect("no form_data?");
	let path_value = map.remove("path").expect("no path in request?");
	let src_value = map.remove("src").expect("no entry in request?");
	let dst_value = map.remove("dst").expect("no entry in request?");
	let path = path_value.text().expect("path not a string?");
	let mut src = path.clone();
	let mut dst = path.clone();
	src += &src_value.text().expect("src not a string?");
	dst += &dst_value.text().expect("dst not a string?");

	rename(&src, &dst).expect("couldn't rename?");

	redirect(&path)
}

async fn mount_endpoint(form_data: Value<()>) -> HttpResponse {
	let mut map = form_data.map().expect("no form_data?");
	let partition_value = map.remove("partition").expect("no path in request?");
	let partition = partition_value.text().expect("path not a string?");

	let dev_path = format!("/dev/{}", &partition);
	let dst_path = format!("./{}", &partition);
	let fstype = SupportedFilesystems::new().expect("can't probe fs?");
	create_dir(&dst_path).expect("couldn't create dir?");
	if let Err(e) = Mount::new(&dev_path, &dst_path, &fstype, MountFlags::empty(), None) {
		remove_dir(&dst_path).expect("couldn't remove dir during err handling?");
		panic!("{}: couldn't mount?", e);
	}

	redirect(&partition)
}

async fn unmount_endpoint(form_data: Value<()>) -> HttpResponse {
	let mut map = form_data.map().expect("no form_data?");
	let partition_value = map.remove("partition").expect("no path in request?");
	let partition = partition_value.text().expect("path not a string?");

	unmount(&partition, UnmountFlags::empty()).expect("couldn't unmount :/");
	remove_dir(&partition).expect("couldn't remove dir?");

	redirect("")
}

async fn upload_endpoint(form_data: Value<PathBuf>) -> HttpResponse {
	let mut map = form_data.map().expect("no form_data?");
	let path_value = map.remove("path").expect("no path in request?");
	let file_value = map.remove("file").expect("no file in request?");
	let mut path = path_value.text().expect("not a string?");
	let orig_len = path.len();
	let file_meta = file_value.file().expect("not a file?");

	path += &file_meta.filename;
	copy(&file_meta.result, &path).expect("couldn't copy?");
	remove_file(&file_meta.result).expect("couldn't remove?");

	redirect(&path[..orig_len])
}

async fn save_file(mut stream: Pin<Box<dyn Stream<Item = Result<Bytes, Error>>>>) -> Result<PathBuf, Error> {
	let temp_file = empty();
	let filename = temp_file.path().to_path_buf();

	let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&filename)
			.expect("did temp_file fail?");

	while let Some(res) = stream.next().await {
		let bytes = res?;
		file.write_all(&bytes).ok().ok_or(Error::FileSize)?;
	}
	file.flush().ok().ok_or(Error::FileSize)?;

	temp_file.leak();
	Ok(filename)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
	let upload_form = Form::new()
		.field("path", Field::text())
		.field(
			"file",
			Field::file(move |_filename, _content_type, stream| {
				async move {
					save_file(stream).await
				}
			}),
		);

	let delete_form = Form::<(), IoError>::new()
		.field("path", Field::text())
		.field("entry", Field::text());

	let rename_form = Form::<(), IoError>::new()
		.field("path", Field::text())
		.field("src", Field::text())
		.field("dst", Field::text());

	let mount_unmount_form = Form::<(), IoError>::new()
		.field("partition", Field::text());

	HttpServer::new(move || {
		App::new()
		.service(resource("/_/upload")
			.route(post().to(upload_endpoint))
			.wrap(upload_form.clone())
		)
		.service(resource("/_/delete")
			.route(post().to(delete_endpoint))
			.wrap(delete_form.clone())
		)
		.service(resource("/_/rename")
			.route(post().to(rename_endpoint))
			.wrap(rename_form.clone())
		)
		.service(resource("/_/mount")
			.route(post().to(mount_endpoint))
			.wrap(mount_unmount_form.clone())
		)
		.service(resource("/_/unmount")
			.route(post().to(unmount_endpoint))
			.wrap(mount_unmount_form.clone())
		)
		.service(Files::new("/", ".")
			.show_files_listing()
			.use_hidden_files()
			.redirect_to_slash_directory()
			.files_listing_renderer(|dir, req| directory_renderer(dir, req))
			.prefer_utf8(true)
		)
	})
		.bind("0.0.0.0:8080")?
		.run()
		.await
}
