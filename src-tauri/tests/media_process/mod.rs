use super::progress_line;
use tokio::io::{AsyncWriteExt, BufReader};

#[tokio::test]
async fn progress_rejects_an_overlong_line_without_waiting_for_newline_or_eof() {
    let (mut writer, reader) = tokio::io::duplex(8192);
    writer.write_all(&vec![b'x'; 4097]).await.unwrap();
    let mut reader = BufReader::new(reader);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        progress_line(&mut reader),
    )
    .await
    .expect("must reject oversized output while the pipe is still open");
    assert!(result.unwrap_err().contains("limit"));
}

#[tokio::test]
async fn progress_handles_multiple_lines_crlf_and_unterminated_final_line() {
    let mut reader = &b"out_time_us=123\r\nprogress=continue\nprogress=end"[..];
    assert_eq!(
        progress_line(&mut reader).await.unwrap().as_deref(),
        Some("out_time_us=123")
    );
    assert_eq!(
        progress_line(&mut reader).await.unwrap().as_deref(),
        Some("progress=continue")
    );
    assert_eq!(
        progress_line(&mut reader).await.unwrap().as_deref(),
        Some("progress=end")
    );
    assert_eq!(progress_line(&mut reader).await.unwrap(), None);
}
