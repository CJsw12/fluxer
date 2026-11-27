use super::*;

#[tokio::test]
async fn capture_status_round_trip_and_forwarding_keep_request_identity() {
    let (reader, mut worker_out) = tokio::io::duplex(4096);
    let connected = serde_json::to_string(&WorkerEvent::Connected).unwrap();
    worker_out.write_all(format!("{connected}\n").as_bytes()).await.unwrap();
    let room = connect_over_streams(
        tokio::io::sink(),
        reader,
        WorkerCommand::ListScreenSources,
        Duration::from_secs(1),
        None,
    ).await.unwrap();
    assert_eq!(room.screen_share_request_id(), Some(0));
    room.set_screen_share(true, true);
    room.set_screen_share(true, false);
    assert_eq!(room.screen_share_request_id(), Some(2));

    let error = Some("No Windows playback device".to_owned());
    let event = WorkerEvent::ScreenShareAudio { request: 1, active: false, error: error.clone() };
    let line = serde_json::to_string(&event).unwrap();
    assert_eq!(serde_json::from_str::<WorkerEvent>(&line).unwrap(), event);
    worker_out.write_all(format!("{line}\n").as_bytes()).await.unwrap();
    let mut rx = room.take_event_rx().unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap(),
        Some(VoiceRoomEvent::ScreenShareAudio { request: 1, active: false, error })
    );
}
