use super::*;

fn write(directory: &Path, name: &str, lines: impl IntoIterator<Item = String>) {
    let mut text = String::new();
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    std::fs::write(directory.join(name), text).unwrap();
}

fn numbered(range: std::ops::Range<usize>) -> impl Iterator<Item = String> {
    range.map(|n| format!("line {n}"))
}

#[tokio::test]
async fn diagnostics_caps_app_tail_by_lines_and_bytes() {
    let directory = tempfile::tempdir().unwrap();

    // By lines: only the newest 200 of 1000, in order.
    write(directory.path(), "app.log", numbered(0..1000));
    let lines = FileLogTail::new(directory.path())
        .last_lines(200)
        .await
        .unwrap();
    assert_eq!(lines.len(), 200);
    assert_eq!(lines.first().unwrap(), "line 800");
    assert_eq!(lines.last().unwrap(), "line 999");

    // By bytes: 100 lines of 1 KiB against a 10 KiB budget yield the newest whole lines only.
    let long = |n: usize| format!("{n:03}-{}", "x".repeat(1019));
    write(directory.path(), "app.log", (0..100).map(long));
    let lines = FileLogTail::with_byte_budget(directory.path(), 10 * 1024)
        .last_lines(200)
        .await
        .unwrap();
    assert!(!lines.is_empty() && lines.len() <= 10, "{}", lines.len());
    assert!(lines.iter().map(|line| line.len() as u64 + 1).sum::<u64>() <= 10 * 1024);
    assert!(lines.iter().all(|line| line.len() == 1023));
    assert_eq!(lines.last().unwrap(), &long(99));

    // A single line larger than the whole budget is never returned partially.
    std::fs::write(directory.path().join("app.log"), "y".repeat(64 * 1024)).unwrap();
    let lines = FileLogTail::with_byte_budget(directory.path(), 1024)
        .last_lines(200)
        .await
        .unwrap();
    assert!(lines.is_empty());
}

#[tokio::test]
async fn rotated_files_fill_the_tail_in_chronological_order() {
    let directory = tempfile::tempdir().unwrap();
    write(directory.path(), "app.log.2", numbered(0..3));
    write(directory.path(), "app.log.1", numbered(3..8));
    write(directory.path(), "app.log", numbered(8..10));
    let tail = FileLogTail::new(directory.path());

    let lines = tail.last_lines(6).await.unwrap();
    assert_eq!(
        lines,
        ["line 4", "line 5", "line 6", "line 7", "line 8", "line 9"]
    );
    let lines = tail.last_lines(200).await.unwrap();
    assert_eq!(lines.len(), 10);
    assert_eq!(lines.first().unwrap(), "line 0");
}

#[tokio::test]
async fn unreadable_active_log_is_an_error_and_odd_bytes_are_tolerated() {
    let directory = tempfile::tempdir().unwrap();
    let tail = FileLogTail::new(directory.path());
    assert_eq!(
        tail.last_lines(10).await,
        Err(DiagnosticsError::Unavailable)
    );

    std::fs::write(
        directory.path().join("app.log"),
        b"first\r\nbroken \xff\xfe bytes\r\nlast",
    )
    .unwrap();
    let lines = tail.last_lines(10).await.unwrap();
    assert_eq!(lines[0], "first");
    assert!(lines[1].starts_with("broken ") && lines[1].ends_with(" bytes"));
    assert_eq!(lines[2], "last");
    assert!(tail.last_lines(0).await.unwrap().is_empty());
}
