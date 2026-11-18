use super::*;
use std::cell::RefCell;
use std::path::PathBuf;

#[test]
fn exec_gets_the_installed_path_and_the_original_arguments() {
    let seen = RefCell::new(None);
    let error = restart_with(
        Path::new("/home/user/bin/backlot"),
        ["--session", "host-a"].map(OsString::from),
        |program, argv| {
            *seen.borrow_mut() = Some((program.to_path_buf(), argv));
            io::Error::other("exec refused")
        },
    );
    assert_eq!(error.to_string(), "exec refused");
    assert_eq!(
        seen.into_inner(),
        Some((
            PathBuf::from("/home/user/bin/backlot"),
            vec![OsString::from("--session"), OsString::from("host-a")],
        ))
    );
}
