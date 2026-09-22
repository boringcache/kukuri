# #1221 通信作業のinventory

基準: `984a491a3430f1da105bfe8ff49a877cbab4690f`。設計: [ADR 0055](../adr/0055-demand-owned-network-work.md)。
状態: **P2調査中**。以下はコードを読んで確認したgroup。NET-AC-1の全件列挙完了とはまだ扱わない。
`.codegraph/` は存在するが `codegraph explore` は「indexなし」を返したため、索引を作成せずrgとsource読みに切り替えた。
全件検索の途中にある `#[cfg(test)]` でファイル後半を捨てない。productionの後にtestがある場合も、
testの後にproductionが続く場合もあるため、module境界とcallerを確認する。

## 確認したgroup

pathの `service/` は `crates/app-api/src/service/`、`cn-runtime/` は
`crates/desktop-runtime/src/community_node/` を指す。全memberの逆引きはP3差分を入れる前に固定する。
intervalは現行値であり新設計の推奨値ではない。

| ID / memberと場所 | trigger・間隔・対象 | 現行の停止 / 連鎖 | 総数への依存 / 配置 | guard・検証 |
| --- | --- | --- | --- | --- |
| N01 `PeerAddrBook::{merged_peers,ranked_peers,set_seed_peers,record_learned_peer}`、`transport/src/peers.rs` | ticket/learn/seed/取得。各serviceのpeer全体 | 各service寿命。docs側はreapplyへ進む | merge/clone/sortが全peer、mapに件数上限なし。P3共通owner | scope別候補、NW-5/6 |
| N02 `RemoteFetchRetryState::{begin,finish}`、`iroh-node/src/remote_fetch.rs` の取得入口 | local miss、retry。全候補。30秒は実行時、接続5秒/転送15秒 | 通常walkは待機者cancel後も継続。in-flight task→permit待機→接続 | 実行8の外に無制限の待機とcooldown走査。P3受付 | NW-2/3/4、LocalOnly |
| N03 `TopicWarmupCoordinator::warmup_peers_once/warmup_peer`、`transport/src/iroh/topics.rs` | topic join/peer追加。direct 250ms〜5秒、relay 1〜10秒 | deadline/neighbor成立。peerごとspawnして並列2のpermit待機 | 候補数に比例するtask。P3所有/停止 | NW-2/5/6 |
| N04 `ensure_hint_topic/extend_active_topic_peers/remove_topic_state`、同上 | hint subscribe/publish、peer変化。全topicへ追加 | receiverはabort、warmupの所有を別途解消する必要。bootstrap集合変化で再join | 全topic×peer、再購読。P3差分 | NW-5/7 |
| N05 docs `reapply_sync_peers` とopen/start/subscribe、`docs-sync/src/iroh_sync.rs` | seed/学習/restore。cached replica集合 | local-onlyはsync昇格しない。registry guard内で再適用 | replica×peer、同期requested全体。P3差分/P4bucket | NW-3/5、LIFE契約 |
| N06 `close_replica_owned/close_replica_under_guard`、`iroh_sync_lifecycle.rs`、`iroh_local_source.rs` | close/revoke/既存source読取り。最大32所有task | caller cancel後も所有。leave失敗隔離、停止task完了 | P1で有界な停止基盤。再監査し直さずP3接続 | NW-7、P1監査/CI |
| N07 `ensure_topic_subscription/spawn_topic_subscription/maybe_restart_*`、`service/timeline_subscription_support.rs` | timeline操作、empty/recovery、欠損本文 | subscription registryのtask終了/明示再起動。docs/blob/withdrawal取得を起動 | topic登録、期限map、背景spawnの累積。P3 | NW-1/3/4/5 |
| N08 `spawn_subscription_task/ensure_joined_private_channel_subscriptions`、`service/private_channels_support.rs` | private参加/表示/restart。recovery tick 1秒 | audience/epoch確認、event/hint/復旧。channel退出で停止 | 全joined/各channelの周期/期限。P3/P4 | NW-3/7/9 |
| N09 `ensure_author_subscriptions_for_rows/spawn_author_subscription`、`service/social_runtime_support.rs` | row表示、author操作、1秒catchup、bootstrap recovery | docs/hint receiverとbootstrap task。著者購読registry | 著者登録累積/各author周期。P3/P4 | NW-1/5、author署名/権限 |
| N10 `rebuild_author_relationships/current_mutual_direct_message_peers/schedule_direct_message_reconcile`、`social_runtime_support.rs/social_helpers.rs` | social変更/startup/DM操作 | 全following/followerとFoF確認→DM reconcile | graph全体の列挙と全subscription差分。P3対象別索引 | NW-8、mutual未確認時拒否 |
| N11 `reconcile_direct_message_subscriptions/spawn_direct_message_subscription/direct_message_topic_snapshot`、`service/direct_messages_subscription_support.rs` | mutual peer単位、2秒outbox tick | pairwise hints、outbox flush、全peers snapshot。registryで停止 | DM相手数×task/timer、outbox全件filter。P3受信route | NW-8/9、ACK/tombstone |
| N12 `session_projection.rs`、`session_display.rs`、UI `SessionVisibility/useSessionDisplay` | 表示observer、window/columnの非表示 | 対象64/observer64、表示futureをcancel、保存前再guard | 既存の有界需要をP3へ接続。参加sessionの寿命とは別 | NW-4/7、既存session manifest tests |
| N13 `run_community_node_session_maintenance_once/start_community_node_session_scheduler`、`cn-runtime/scheduler_support.rs` | runtime起動、15秒、設定node全体 | `MaintenanceTasks`がjobごとのfutureを所有、Skip、shutdown停止 | 所有は既存。node全体再列挙とstatusからselfheal。P3due索引 | NW-6、mixed認証/同意 |
| N14 `maybe_self_heal_community_node_connectivity/repair_community_node_connectivity`、`cn-runtime/reconnect_support.rs` | sync status不健全、期限付きbackoff | reconnect成功でreset、seed/購読再適用へ連鎖 | 全active/全ready nodeへ波及。P3差分と観測 | NW-5/6/7 |
| N15 `observe_sync_status_once/start_sync_status_observer`、`runtime/sync_status_observer.rs` | 3秒、全体snapshot | runtime shutdown。変更snapshotだけemit | 全peers/topic/nodeの再集計。P3増減集計 | NW-1/6 |
| N16 `commands/background_notifications.rs::spawn` とdispatch | 受信event + 60秒fallback、trayでも継続 | runtime/アプリ寿命。通知一覧読取り→OS dispatch | 通知全件、同timestampのID集合に上限なし。P3cursor | NW-8、OS設定/二重toast |
| N17 `cn-indexer/src/participant.rs/scheduler.rs` | scope設定、docs/hint event、再評価 | post job台帳1,024、lease dropでcancel記録 | 現行の有界schedulerを再利用。scope境界/差分はP4 | CN-AC-1〜4、NW-9/10 |
| N18 UI `shell/data/timelineMerge.ts`、`slices/timeline.ts`、`viewModels/useTimelineViewModels.ts` | refresh/event/遡り/列切替 | UI observer寿命、再取得をapp-apiへ渡す | 累積窓のmap/merge/setState・cacheをP3で索引/窓化 | UI-AC群、100/1k/10k、focus/scroll/draft |
| N19 `join_live_session/stop_live_presence_task`、`live.rs/service/live_game_support.rs` | 参加中liveごと10秒、TTL30秒 | ended判定またはleave/shutdownでtask停止、hint送信とlocal presence更新 | 参加数に比例。P3の参加leaseへ登録、hiddenでは停止しない | NW-4/7、live終了契約 |
| N20 `spawn_owner_dome_heartbeat_task`、`dome_hosting.rs` | host sessionごと5秒、Delay | session不在/ID変更/署名失敗で終了。handleのownerなし | host session数、送信失敗時continue。P3 task所有 | NW-6/7、Dome lease/draining |
| N21 `dome_connection_support.rs` のconnection終了/blocked再評価 | 所有者の終了操作、drain期限待ち | caller future内sleep後に状態再読込み。全host runtimeへdrain | 全joined context/connection走査。P3索引差分、P4state配置 | 世代/owner guard、drain中cancel |
| N22 `wait_for_private_channel_epoch_snapshot`、`object_persistence_support.rs` | epoch参加/rotation、50ms間隔・全体10秒 | caller取消/期限で停止。metadata/policy/participantsを再取得 | participant全体の反復。P4個別owner/recipient確認 | NW-9、旧epoch grant経路 |
| N23 `spawn_reply_target_reflection`、`reply_target_support.rs` | 未反映の返信先、台帳受付後 | spawn後にpermit待機、所有handleなし、docs LocalOnly→本文remote | 台帳は既存、task所有とscope別取得はP3統合 | NW-2/4/9、署名/LocalOnly維持 |
| N24 `MissingBodyLedger`、`hydration_limits.rs` | 欠損本文、5秒/30秒/120秒/600秒、最大8試行 | RAIIで失敗記録、最大4実行/4,096記録 | 台帳は有界。上限は共通予算へ統合し履歴をリセットしない | NW-2/4、既存missing-body tests |
| N25 `runtime/mod.rs` の通知event転送、`host/mod.rs::replace_event_task/restore_desired_subscriptions` | account起動/restart、notify event | host転送taskは置換/shutdownでabort。runtime通知転送はdetached loop | desired購読全件復元と旧runtime task寿命。P3owner/復元索引 | NW-7/8/10 |
| N26 `iroh-node/src/node.rs::apply_relay_config/shutdown`、`transport/src/discovery.rs/iroh/discovery.rs` | 起動/relay設定、endpoint.online待機 | node shutdownは所有付き。online待機はdetached | endpoint世代単位。P3で監視taskを所有し重複設定をno-op | NW-6/7、既存node終了契約 |
| N27 `cn-runtime/requests_support.rs/session_runtime_support.rs` | auth/consent要求、token/heartbeat/rendezvous/metadataの期限、設定変更 | HTTP timeoutと401再認証。ready node/seed集合を再適用 | 全ready node/購読/seedの再合成。P3差分/due索引 | NW-5/6、mixed node auth/consent |
| N28 `cn-core/src/rendezvous.rs::heartbeat`、`cn-user-api/handlers/bootstrap.rs::topic_rendezvous_heartbeat` | 認証・同意後のjoins/refreshes/leaves | request期限。Redis TTL、期限切れpeerを照会中に削除 | topicごとのSMEMBERS/sort/peer GETが全登録peer比例。P3有界cursorとTTL索引 | NW-2/5/8、auth/endpoint binding |

## 上流APIで確認した前提不足

lockの対象はiroh 1.0.3、iroh-gossip 0.101.0、iroh-blobs 0.103.0、
iroh-docs 0.101.0（pin `e7233d14853cb4db9966e30050bac1e689cdeec8`）。

| ID | source / 動作 | P3で満たす必要がある契約 |
| --- | --- | --- |
| U01 | iroh-docs `engine/live.rs::start_sync` は保存済sync peerを引数peerへappendし、`join_peers`で各peerの同期を起動 | 選択peer限定の開始API。leave/startだけでは修正にならない。保存peerは `PEERS_PER_DOC_CACHE_SIZE=5` で既に有界だが、選択外の5件を再開し得る |
| U02 | 同 `leave` はsync停止とgossip quitを行う。進行中syncのJoinSetとneighbor経由のsync受付は別経路 | 停止応答後の旧世代のnetwork/保存とneighborによる選択外開始を制御。P1 closeの再検証ではなくowner統合のdelta |
| U03 | iroh-gossip `api.rs` は `join_peers` を公開し個別peer削除APIなし。sender/receiverの両方dropでtopic leave | 影響topicの全handle/task所有と解放。別topicを巻き込まず、残存sender cloneを残さない |
| U04 | iroh-gossip `proto/state.rs` はpeer切断を全稼働topicへ通知 | 対象topic数を有界にし、休止topicを内部stateへ残さない。意味上の全登録topicとは分ける |
| U05 | irohのendpoint IDは `iroh-node/src/node.rs` が永続鍵から復元する。DHTはendpoint IDを解決 | 既存endpointは署名bindingを再利用可能。鍵再生成時は旧bindingを有効扱いしない |
| U06 | iroh `remote_state.rs` のactorは接続/処理がない状態60秒で終了し、`remote_map.rs::cleanup`がsenderを除去する。mapped address表は別 | actor終了だけでaddress台帳の回収も済むとは扱わない。内部接続数とmapped addressの上限/解放を確認 |
| U07 | gossip hyparviewの既定active viewは5、passive viewは30、topicごとの有界集合 | per-topic上限を既存成果として再利用。全topicの合計/共有接続はowner予算と整合させる |
| U08 | iroh-docs `on_replica_event -> start_download` はremote entryから独自downloader/task/hash provider台帳へ進む。app-apiのblob受付を通らない | docsのdownload policyとownerへの取得委譲を設計する。neighbor content-ready経路と未実行hash台帳も確認し、app-api側8枠だけで全取得有界としない |
| U09 | iroh-gossipの既定message上限は4,096bytes。private replica IDの最大形はこれを超え得る | 最大長locatorをhintへ丸ごと入れない。固定サイズの暗号化参照から、署名provider/epoch制限付きで有界payloadを取得するwireを先に検証 |

irohのmapped address表は `mapped_addrs.rs::AddrMap` の正引き/逆引きHashMapであり、actorの終了と別の寿命を持つ。
内部接続とmapped addressの容量・停止、docsの進行中sync取消、blobs downloader内部の受付は残確認。
外側のSemaphoreや時間bucketだけでこれらも有界と主張しない。

## Sensitive sinkの逆引き

### 追加したbinding交換（P2実証、常設runtime未登録）

| ID | 入口 → helper → sink | guard / 停止 | 対応contract |
| --- | --- | --- | --- |
| N29 | `fetch_receive_endpoint_binding` → endpoint connect/open_bi/read → `verify_for` | 候補1件、受付deadline、1,024byte、QUIC remote ID照合。完了/失敗/cancelでclose | `receive_binding_exchange_uses_authenticated_endpoint_without_cn`、`receive_binding_replay_from_another_endpoint_is_rejected`、`receive_binding_cancel_closes_the_connection` |
| N30 | Router → `ReceiveBindingProtocol::accept/serve` → binding write | try_acquireで2要求、2秒、1byte request。失効検証、scope取得なし | `receive_binding_full_server_rejects_instead_of_waiting` |
| N31 | `ReceiveBindingProtocol::new/replace` → binding更新 | account/endpoint/署名/時刻/単調更新。鍵は保持しない | `receive_binding_replacement_cannot_switch_account_or_endpoint`、core binding tests |

この3行は§4.1の限定実装範囲。署名bindingを取り交わすだけでは、account routeのgossip配信、
private capsule、旧DM outboxの移行を達成したとは扱わない。

| sink | 既知の入口group | 必須の支配guard / 禁止副作用 | 差分前の残確認 |
| --- | --- | --- | --- |
| endpoint connect / gossip join,publish | N01/03/04、N07〜11、N13/14 | owner受付、protocol/scope候補、同意、endpoint世代。無関係/休止topicのI/O 0 | upstream内部の接続保持/再試行、各public trait caller |
| docs open/start_sync/query/leave | N05〜09/17、CN source reader | LocalOnlyをsyncへ昇格しない。private能力/世代。閉鎖中の再open禁止 | writer/controllerとlegacy migration caller |
| blob remote fetch / persistence | N02/07/08/11/12/17 | mode/bytes/scope/取得gate、完了時再guard。private hashを無許可peerへ出さない | media、live/game/Dome、reply targetの全caller |
| CN HTTP / token / seed適用 | N13/14、requests/session support | node別auth/consent、401時停止、自己修復で別nodeを昇格しない | request helper全caller、明示設定setter/restore |
| notification/DM/outbox DB mutation | N10/11/16、新受信入口 | 署名、mutual/audience、dedupe/既読、ACK、tombstone | notification candidateから保存まで、backup migration |
| namespace/blob/projection GC | N06、P4回収 | 保護objectと依存参照、cursor、部分失敗の冪等再開 | GC/backup/restore/export/import全caller |

## 未分類を解消する範囲

次はgrep候補を拾っただけで、停止・連鎖までの確認が残る。全件inventory完了と主張しない。
一つずつ別タスクを増やさずP2でgroupへ統合し、既存の固定ACに関連しないものは理由付きで対象外へ分類する。

1. `app-api` の `dome_connections/dome_hosting/live/private_channels/timeline`、
   `service/{dome_connection_support,live_game_support,object_persistence_support,reply_target_support,mod}`。
   active sessionのheartbeat/終了、bounded hydration、共有read helperの再取得連鎖。
2. `desktop-runtime` の `host/{accounts,mod}`、`runtime/{mod,private_channels_game_api}`、`stack`、
   `community_node/{requests_support,session_runtime_support,session_state_support,http_client_support}`。
   account切替、stack probe/rebuild、認証/metadata/rendezvousの期限・全setter。
3. `iroh-node/src/node.rs`、`transport/src/{discovery,iroh/discovery,iroh/endpoint}`、
   pin済みiroh/iroh-docs/iroh-gossip/iroh-blobsの内部task・retry・peer cache。
4. CN participant/workerの登録点からingest/media/sourceまで、bucket writer/read/GCの全入口。
   [replica read inventory](replica-read-inventory.md)を再利用し、重複した全件調査をしない。
5. Tauriの `desktop_lifecycle/state/lib`、`commands/{link_preview,device_backup,os_notification}`、
   UIのpoll/subscribe/object URLを含む登録点。OS thread/単発blocking処理とnetwork retryを分ける。

`FakeTransport`、各tests/fixture内のsleep/spawn、エラーDTOの`retry_after_seconds`フィールドは
production schedulerとして数えない。ただしproductionと同一ファイルの場合はmodule境界で除外する。
updater、ファイルdialog、identity export、外部URL起動はそれだけで本Issueの新要件にしない。

## 終了条件

P2終了時に各groupのmember・caller・停止・連鎖と上流APIの実現可能性を確認し、未分類を0へ更新する。
固定AC/INVARとNW transitionへの対応を独立監査する。P3/P4では差分を入れたgroupだけを更新し、
同一headの成功監査と無関係な全suiteを繰り返さない。詳細な作業状態は#1221本文へ記録する。
