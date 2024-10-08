use std::{
    ffi::OsStr,
    fs::{read_dir, read_to_string, DirEntry},
    path::PathBuf,
    str::FromStr,
};

use busd::configuration::Configuration;

#[test]
fn find_and_parse_real_configuration_files() {
    let mut file_paths = vec![
        PathBuf::from("/usr/share/dbus-1/session.conf"),
        PathBuf::from("/usr/share/dbus-1/system.conf"),
    ];

    for dir_path in ["/usr/share/dbus-1/session.d", "/usr/share/dbus-1/system.d"] {
        if let Ok(rd) = read_dir(dir_path) {
            file_paths.extend(
                rd.flatten()
                    .map(|fp| DirEntry::path(&fp))
                    .filter(|fp| fp.extension() == Some(OsStr::new("conf"))),
            );
        }
    }

    for file_path in file_paths {
        let configuration_text = match read_to_string(&file_path) {
            Ok(ok) => ok,
            Err(_) => continue,
        };

        Configuration::from_str(&configuration_text).unwrap_or_else(|err| {
            panic!("should correctly parse {}: {err:?}", file_path.display())
        });
    }
}
