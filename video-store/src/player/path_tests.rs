use super::absolute_path;

#[test]
fn unix_mount_joins_with_slash() {
    assert_eq!(
        absolute_path("/mnt/33/peter", "cb/a.mp4"),
        "/mnt/33/peter/cb/a.mp4"
    );
}

#[test]
fn drive_mount_uses_backslashes() {
    assert_eq!(
        absolute_path("F:\\", "d/chaturbate/x.mp4"),
        "F:\\d\\chaturbate\\x.mp4"
    );
}
