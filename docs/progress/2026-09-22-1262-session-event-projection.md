# #1262 session の到着イベントによる反映

## 現在の状態と承認範囲

- 状態: 実装・ローカル検証完了、正式監査待ち。CI・mergeは未完了。
- 基準: `dba0881da72f8bca974f3e9d76c4d13c434894e0`
- 統合基準: `59900e54`（#1292 / PR #1295）。作業中にmainへ入ったchannel別の上限つき一覧・単件lookupを取り込み、旧topic全件getterへ戻さない。
- Scope revision: [#1262](https://github.com/kukuri-app/kukuri/issues/1262) の `2026-09-22-v2`。AC-1〜4 / INVAR-1〜2を維持。
- リスク区分: C。表示状態から外部取得を制御し、共有検証 helper を変更するため。PR head の独立監査を別工程に置く。
- 利用者が計画・実装・Issue関連作業・コミット・PR・CI成功後のマージを承認済み。
- 表示対象は表示範囲内の一覧カードと開いている詳細。topic の購読、選択、一覧APIの呼出しだけでは取得しない。
- CodeGraph: `.codegraph` は存在するが CLI は索引を利用できず、通常の検索へ切替（索引は作成しない）。

## 作業と条件

| ID | 条件 | 成果物・検証 | 依存 |
| --- | --- | --- | --- |
| T1 | AC-1/2 | 拒否 state の読み出し回数と後続 event の進行の失敗testを修正前に確認 | なし |
| T2 | AC-1/2, INVAR-2 | 確定的拒否・docs待ち・manifest欠損・反映済みを分離。sessionのtimer retry廃止 | T1 |
| T3 | AC-2/4, INVAR-1 | state/envelope両順序の個別反映。欠損・拒否からのcatch-up/re-syncを抑止 | T2 |
| T4 | AC-3/4 | 表示登録・解除、上限つきmanifest取得、完了・取消・shutdown | T2 |
| T5 | AC-3, INVAR-1/2 | 起動・一覧・操作のcaller整合。候補表示と検証済みprojectionを区別 | T3/T4 |
| T6 | 全条件 | ScoreGame pointer/lock契約、既存test・ADR・記録の更新 | T3〜T5 |
| T7 | 全条件 | ローカル必須検証とPR headの独立監査 | T6 |

## 固定 inventory の補完

| ID | 入口・trigger | helper | 副作用とguard | 遷移 |
| --- | --- | --- | --- | --- |
| INV-1 | docs state/envelope event | hydrate_subscription_doc_event | 定数のlocal読み・検証後のprojection | TR-1/2/4 |
| INV-2 | SessionChanged hint | hydrate_subscription_hint | 定数の読み。欠損・拒否でretryしない | TR-1/2 |
| INV-3 | public/private購読loop | 上記2入口 | 後続eventをsessionの取得で止めない | TR-2 |
| INV-4 | startup/catch-up/一覧補完 | catch_up_sessions | 非表示manifestのremote取得0 | TR-5 |
| INV-5 | GUIの表示/解除、CLIのlist_session_candidates/set_session_display、取得完了/取消/shutdown | session_display、session_projection、runtime/Tauri/CLI wrapper | scope確認後だけ取得。同時2・表示/待機各64・同一replica/session key/manifestの組につき最大3試行、timer再試行なし | TR-3/6/7 |
| INV-6 | join/end/game更新 | load_verified_* | 未検証のstate/manifestを操作に使わない | TR-8 |
| INV-7 | ScoreGameの取得完了 | room lockとprojection commit | 現在pointer確認、古いcandidateで上書きしない | TR-7/8 |

列挙方法: hydrate_*_from_key、verify_*_record、load_verified_*、fetch_manifest_blob、session_projection_retry_*の全callerを逆引き。
Sensitive sink: manifest remote fetch、blob状態/キャッシュ更新、session projection commit、session操作の永続mutation。

## 遷移

IssueのTR-1〜4を維持し、実装範囲の補完を次に固定する。

| ID | 事前状態・契機 | 期待結果 | 禁止する副作用 |
| --- | --- | --- | --- |
| TR-5 | 非表示、初回/空一覧/再起動 | local情報で欠損状態を維持 | 非表示manifestのremote取得 |
| TR-6 | 取得失敗/重複event/枠超過 | 上限内、利用者は操作を継続 | timer retry、無限task、再描画での予算reset |
| TR-7 | 取得中の非表示化/退出/shutdown/pointer更新 | 解除し無効な完了を無視 | 古いcandidateのcommit、解除後の新規取得 |
| TR-8 | 複数session/public-private混在/同timestamp更新 | 各対象のguard・lockを維持 | 他対象への許可の伝播、未検証操作 |
| TR-9 | 通知対象bytes未着、同じkeyに不正recordあり→ContentReady | 通知content hashを保持し、到着後に対象だけ反映・画面へ通知 | 不正recordがあることによる到着待ちの消失、窓への依存 |
| TR-10 | 既存カードでviewportが埋まる→未取得IDの詳細を開く | URLの対象を保持し、既存focus/scroll経路で候補を表示位置へ移す | 一覧にないだけで対象IDを消すこと |

## UIの変更分類と維持する挙動

不具合修正。対象はlive/game/Domeの既存一覧・詳細で、manifest未取得でも表示と操作を継続できることが目的。
初期取得・未取得候補・取得失敗・検証済み・非表示・scope退出を扱う。候補には参加/終了/score更新の操作を付けず、通常のprojectionと区別する。
検証済みカードのlayout、draft、Column scope、通常のfocus/scrollを維持し、新規style/tokenは追加しない。
未取得の詳細IDはbounded listの不在だけでは無効と判定せず、既存focus hookが対象候補へ移動する。
候補の増減/取得完了は既存last_sync_ts通知へ流し、表示中scopeだけを再取得する。再描画や同一候補集合では再通知しない。
JA/EN/zh-CNの不足表示を用意した。Windowsではvisual commandは操作smokeであり、Linuxのpixel baseline一致とは扱わない。

## 必須検証

`cargo xtask check`、`cargo xtask test`、`cargo xtask app-api-slow-test`。
frontend/IPC変更は`cargo xtask desktop-ui-check`、`cargo xtask tauri-test`、`cargo xtask e2e-smoke`。
関連live/game永続化・private channel scenario、#1252の検証test、game_projection_freshness、hint_rehydration。
件数と待ちの上限は回数・制御したfutureでassertし、所要時間の閾値は使わない。

## 証跡

- T1: 修正前の`rejected_live_session_reads_state_once` / `rejected_game_session_reads_state_once`は、期待1回に対し20回で失敗（2 failed）。修正後は同じtestで1回となり成功。
- `session_event_progress` 13件成功。表示限定の取得、複数hashの予算、同時2件、待機取消で予算を消費しないこと、candidate数1,000/10,000/100,000でも台帳64件・非表示取得0、private退出、live巻戻り防止、content hashによる後着反映を確認。
- `content_ready_session_outside_the_window_notifies_the_visible_list`成功。実際の購読notice loopで窓の外のsessionを反映し、last_sync_tsを更新する。
- #1252の`hydration_integrity_sessions*` 20件成功。ScoreGameのlock内比較を止めるtestは、最初のstate読みもLocalOnlyになったため停止点を区別した（同一room writerを止めるassertionは維持）。当該2件成功。
- frontend全体: 249 files / 1,998 tests成功（`pnpm test -- --maxWorkers=2`）。先行の既定並列実行では2件のtiming failureがあり、成功と混同しない。後続deltaのroutes/SessionVisibilityは19件成功。
- browser: 全体373成功/1失敗。失敗した既存Metaverse layout（en/dark/1 span）は同じコードで単独再実行し成功。新規表示gate/取得完了/窓外詳細のbrowser test 3件も成功。最終全体374件成功。
- Storybook build成功。visual操作smoke 44件成功（Windows、pixel比較なし）。Tauri lib test 77件成功。
- `check`成功（fmt、Clippy全target、Tauri compile、frontend lint/typecheck）。後続deltaの再確認を行う。
- slow test先行実行: 457成功/3失敗。既存の実peer session testが表示要求なしで取得する旧前提だったため、表示要求を追加して同じ到達/score/状態のassertionを維持した。
- slow test次回: 462成功/1失敗。friend-plusのIroh codec内panic。単独実行でも再現し、main `59900e54`の比較checkoutでは成功した。退出への追加処理だけを戻す比較で成功したため、参加状態を先に削除し、取得開始/commitと退出を同じ排他で制御する形へ修正。修正後の同一testと全slow suiteが成功した。
- 比較checkoutと共通targetを使った後、古いBlobService型の成果物が混入した。`cargo clean -p kukuri-blob-service -p kukuri-app-api`で該当成果物だけを破棄し、現行コードから再buildする。
- 更新前のxtask binaryでのprivate channel scenarioは表示要求を含まずtimeout。無効な検証として扱い、最新runnerで再実行する。post/live/game永続smokeの先行実行は成功。
- CLI parityで追加2commandの未分類/件数の不一致を検出し、handler・schema・対応表（168入口）とscope revisionを同期。CLIの実peer consumerも表示対象を明示する形へ更新し、認可拒否/伝播のassertionは維持する。

### 初回実装49fb601dの検証結果（監査修正前）

| 検証 | 結果 |
| --- | --- |
| `cargo xtask rust-test` | 1,366 passed / 5既存skip、doctest成功。CLI parity・実peerの非owner game更新拒否を含む |
| `cargo xtask app-api-slow-test` | 464 passed（`RUST_TEST_THREADS=2`）。Friends+をskipする環境変数は設定していない |
| frontend `pnpm test -- --maxWorkers=2` | 249 files / 1,998 passed。後続routing/visibility差分は19件+102件のtargeted test成功 |
| browser `pnpm test:e2e:browser -- --workers=2` | 374 passed。表示範囲、取得完了後の一覧更新、既存カードより後ろの未取得詳細を含む |
| `desktop-storybook` / `desktop-visual-test` | build成功 / 44操作smoke成功（Windowsではpixel比較skip） |
| `cargo xtask e2e-smoke` | 最新runnerでpost永続化6 step成功 |
| `scenario desktop_smoke_live_session_persist` | 最新runnerで8 step成功 |
| `scenario desktop_smoke_game_room_persist` | 最新runnerで7 step成功 |
| `scenario private_channel_invite_connectivity` | 最新runnerで11 step成功（実peer、private live/gameの表示要求を含む） |
| `cargo xtask check` / `cargo xtask tauri-test` | 最終差分で成功 / 77 passed |

Linux専用CLI process E2EとLinux pixel baseline比較はWindowsで実行できないためCIで確認する。CLI handler・schema・伝播の契約はWindowsのrust-testでも実行した。テストが生成した今回無関係な#992画像は差分から除く。

## 配置とbaseline更新の理由

新規処理はsession_projection/session_displayと表示hookへ分離した。service/mod.rsの依存注入・shutdown、既存購読loopの入口判定、runtimeApiのIPC登録は既存の登録点への必要な追加であり、このために無関係な構造整理を行わない。
oversized baselineはこれら登録点の増加を明記して更新し、更新commandが検出した既存の縮小も反映する。新規の手書きファイルは1000行未満。

## 独立監査と完了

予備レビューの指摘（複数recordによる予算reset、取消待ちでの予算消費、private退出時の解除、liveの古いcommit、ContentReady通知対象の区別、詳細の表示位置）を修正しtestへ対応した。
正式監査はPR head固定後に実施する。ローカル検証・必須CI・独立監査PASS・merge後の対象surface一致を満たすまでCloseしない。

## 独立監査B-1とmain統合の修正

49fb601dの正式監査はFAIL（inventory 7件、適合6件、INV-5不適合1件、未分類0）。B-1はAC-3/AC-4/INVAR-1、TR-3/6/7に対応するExisting-gap。外側の取消後も既存single-flightの別taskが通信・保存を続け、遅い取得結果が反映されなかった。

旧single-flightを使った取消testは保存回数1（期待0）で失敗した。表示専用のowned futureを導入し、共通walk枠の入場後に試行予算を消費する。表示専用取得はephemeral bytesを返し、保存とprojectionは表示/退出の排他内で行う。共通枠待機を含む30秒の予算内で完了を待ち、取消時には実QUIC streamを閉じる。既存MissingBodyのsingle-flight契約は維持する。

修正後のremote_fetch 10件、session_event_progress 14件、実QUIC取消testが成功。共通枠待機中の取消では予算を使わず、従来の5秒を超える取得もevent追加なしで反映・通知される。

main fa9496a5（署名revision #1260、manifest hash検証 #1261を含む）を統合。live選択が未署名updated_atを使う競合をlive_session_selection_uses_signed_revisionの失敗（Live / Ended不一致）として再現し、署名revisionでの比較へ修正。同testとsession_manifest_fetch 8件が成功。修正後の全体検証とdelta監査を続ける。

### delta独立監査の現在判定

対象2583758770e87dc598b70e471a5151318174f493、Scope revision 2026-09-22-v2、区分C。別コンテキストで取得APIの全callerと保存sinkを追跡しPASS。inventory合計7/適合7/不適合0/未分類0、blocker 0。49fb601dのB-1は解消済み。

監査者が対象headで生成済みのbinaryを直接実行し、session_event_progress14、remote_fetch10、local_status2（実QUIC取消を含む）、session_manifest_fetch8、hydration_integrity_sessions25、game_projection_freshness15が成功。署名revision/hashのmain統合も確認した。対象surface未変更のGUI/IPC/CLI・ContentReady・startup/catch-upは前回監査証跡を継承する。全体ローカル検証・CI・merge後tree比較は別条件として継続する。

### 25837587のローカル最終検証

- cargo xtask check成功（fmt/Clippy全target/Tauri compile/frontend lint/typecheck）。
- rust-test: 1,385 passed / 5既存skip、doctest成功。
- app-api-slow-test: 478 passed（RUST_TEST_THREADS=2、Friends+ skipなし）。
- tauri-test: 77 passed。
- main統合のUI delta: 4 files / 34 passed。変更のないUI surfaceは初回全体1,998件・browser374件・visual44件・Storybook成功の証跡を継承。
- 最新runnerでe2e-smoke（投稿6step）、live永続化8step、game永続化7step、private_channel_invite_connectivity11stepが全成功。
- oversized-files、ipc-types --check、git diff --check成功。

この追記は検証記録のみで監査対象の実装surfaceを変更しない。CIとmerge後tree比較を残す。

### Linux CLI件数assertionの追従

64c1c961のCIはLinux専用daemon testの固定件数148で失敗（実際150）。今回追加したsession表示2commandを期待値へ反映し忘れていた。OS共通registry testへ同じ148のassertionを追加し、Windowsでも150/148の失敗を再現した。両方を150へ更新後、CLI lib全31件、fmt、diff checkが成功。production code/IPC schema/動作の変更はない。Linux daemon/process testはCIで最終確認する。
