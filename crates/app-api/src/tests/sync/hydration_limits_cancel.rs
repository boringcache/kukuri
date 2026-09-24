use super::*;

#[tokio::test]
async fn cancelled_key_hydration_does_not_write_a_missing_projection() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let blobs = Arc::new(HangingBlobService {
        gate: Some(gate.clone()),
        ..Default::default()
    });
    let stored = blobs
        .put_blob(b"late key body".to_vec(), "text/plain")
        .await
        .unwrap();
    let docs = Arc::new(CountingDocsSync::default());
    let keys = generate_keys();
    let topic = TopicId::new("kukuri:topic:cancelled-key-hydration");
    let envelope = persist_test_post(
        docs.as_ref(),
        None,
        &keys,
        &topic,
        PayloadRef::BlobText {
            hash: stored.hash,
            mime: "text/plain".into(),
            bytes: stored.bytes,
        },
        Vec::new(),
        None,
    )
    .await;
    let store = Arc::new(MemoryStore::default());
    let transport = Arc::new(StaticTransport::new(PeerSnapshot::default()));
    let app = Arc::new(app_service_from_dependencies(
        store.clone(),
        store.clone(),
        transport.clone(),
        transport,
        docs,
        blobs.clone(),
        keys,
    ));
    let hydration = {
        let app = app.clone();
        let topic = topic.clone();
        let object_id = envelope.id.clone();
        tokio::spawn(async move {
            crate::service::hydrate_object_in_topic_with(
                &app.services,
                topic.as_str(),
                &topic_replica_id(topic.as_str()),
                &object_id,
                None,
                DocFetchPolicy::LocalOnly,
                crate::service::BodyFetch::Bounded,
            )
            .await
        })
    };
    timeout(Duration::from_secs(2), async {
        while blobs.fetches.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("key hydration fetch starts");
    app.shutdown().await;
    gate.add_permits(1);
    assert_eq!(
        hydration.await.unwrap().unwrap(),
        crate::service::ObjectHydration::Missing
    );
    assert!(
        store
            .get_object_projection(&envelope.id)
            .await
            .unwrap()
            .is_none()
    );
}
