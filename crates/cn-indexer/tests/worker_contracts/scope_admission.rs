use super::*;

#[tokio::test]
async fn worker_admits_at_most_32_legacy_scopes_from_a_larger_supported_set() -> Result<()> {
    let Some(admin_url) = integration_test_admin_database_url() else {
        return Ok(());
    };
    let database = TestDatabase::create(admin_url.as_str(), "cn_legacy_scope_admission").await?;
    let pool = connect_postgres(database.database_url.as_str()).await?;
    initialize_database(&pool).await?;
    for index in 0..80 {
        add_supported_topic(
            &pool,
            IndexScopeKind::PublicTopic,
            &format!("topic-{index:02}"),
        )
        .await?;
    }
    add_supported_topic(&pool, IndexScopeKind::PrivateChannel, "secret-room").await?;
    register_channel_secret(&pool, &cipher(), "secret-room", TEST_NAMESPACE_SECRET).await?;
    add_supported_topic(&pool, IndexScopeKind::PrivateChannel, "no-secret").await?;
    let docs = Arc::new(MemoryDocsSync::default());
    let state = Arc::new(IndexerRuntimeState::default());
    let projection = Arc::new(MemoryIndexProjection::default());
    let (participant, _) = participant_with_docs(&pool, docs.clone(), &projection, &state);
    let selector = participant.clone();
    let handle = IndexerWorker::new(
        participant,
        docs,
        state.clone(),
        fast_config(Duration::from_secs(120)),
    )
    .spawn();
    wait_until("first scope-admission pass", || {
        let state = state.clone();
        async move { state.snapshot().last_pass_duration_ms.is_some() }
    })
    .await;
    assert!(
        state.snapshot().opened_scopes <= 32,
        "legacy worker must leave room for the 32-scope public bucket reader"
    );
    mark_index_demand(&pool, IndexScopeKind::PublicTopic, "topic-79").await?;
    mark_index_demand(&pool, IndexScopeKind::PrivateChannel, "secret-room").await?;
    mark_index_demand(&pool, IndexScopeKind::PrivateChannel, "no-secret").await?;
    let mut visited = std::collections::HashSet::new();
    for _ in 0..3 {
        let selected = selector
            .selected_scopes_at(chrono::Utc::now().timestamp())
            .await?;
        assert!(selected.len() <= 32);
        assert!(selected.iter().any(|scope| scope.id == "topic-79"));
        assert!(selected.iter().any(|scope| scope.id == "secret-room"));
        assert!(!selected.iter().any(|scope| scope.id == "no-secret"));
        visited.extend(selected.into_iter().map(|scope| scope.id));
    }
    assert_eq!(
        visited.len(),
        81,
        "fair cursor must reach all eligible scopes"
    );
    handle.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn rotating_out_a_supported_scope_keeps_its_indexed_post() -> Result<()> {
    let Some(admin_url) = integration_test_admin_database_url() else {
        return Ok(());
    };
    let database =
        TestDatabase::create(admin_url.as_str(), "cn_scope_rotation_keeps_index").await?;
    let pool = connect_postgres(database.database_url.as_str()).await?;
    initialize_database(&pool).await?;
    for index in 0..80 {
        add_supported_topic(
            &pool,
            IndexScopeKind::PublicTopic,
            &format!("topic-{index:02}"),
        )
        .await?;
    }
    let docs = Arc::new(MemoryDocsSync::default());
    let post = persist_post(
        docs.as_ref(),
        &kukuri_docs_sync::topic_replica_id("topic-00"),
        &TopicId::new("topic-00"),
        "still indexed after rotation",
    )
    .await;
    let state = Arc::new(IndexerRuntimeState::default());
    let projection = Arc::new(MemoryIndexProjection::default());
    let (participant, entries) = participant_with_docs(&pool, docs.clone(), &projection, &state);
    let handle = IndexerWorker::new(
        participant,
        docs,
        state.clone(),
        fast_config(Duration::from_secs(1)),
    )
    .spawn();
    wait_until("initial topic-00 index", || {
        let projection = projection.clone();
        let post = post.clone();
        async move {
            projection
                .contains_object(IndexScopeKind::PublicTopic, "topic-00", &post)
                .await
                .unwrap_or(false)
        }
    })
    .await;
    wait_until("topic-00 rotated out", || {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT last_scope_id FROM cn_index.legacy_scope_cursor WHERE id = TRUE",
            )
            .fetch_one(&pool)
            .await
            .is_ok_and(|cursor| cursor == "topic-63")
        }
    })
    .await;
    assert!(entries.contains(IndexScopeKind::PublicTopic, "topic-00", &post));
    assert!(
        projection
            .contains_object(IndexScopeKind::PublicTopic, "topic-00", &post)
            .await?
    );
    assert!(state.snapshot().opened_scopes <= 32);
    handle.shutdown().await;
    Ok(())
}
