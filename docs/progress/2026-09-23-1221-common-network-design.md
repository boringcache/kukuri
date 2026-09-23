# #1221 共通通信設計の判断反映

## 現在の成果

- 基準: `984a491a3430f1da105bfe8ff49a877cbab4690f`。
- P1は完了済み。P2は進行中であり、製品コード・writer・受信経路は切り替えていない。
- ユーザー決定: D2は現在の受信対象を維持。D10は更新案内後bucket切替時に旧版との新着相互運用を終了。
- [ADR 0055](../adr/0055-demand-owned-network-work.md)へowner/需要/容量/停止/受信/移行を記録。
- [inventory](../architecture/network-work-inventory.md)へ既知の28groupと上流9項目、残確認を記録。
  全件列挙完了・未分類0とは扱わない。
- ADR 0054の互換方針と旧子Issueの管理参照を同期し、docs入口・runbookから新設計へ接続した。

## 限定した独立設計レビュー

区分C、Scope revision `2026-09-22-consolidated-v2`。
担当 `/root/design_audit` は実装に関与せず、ADR 0055 §1〜5の未commit草案を
現行ADR 0020/0023/0054と照合した。これは固定PR headの実装監査や全P2完了監査ではない。

| 指摘 | 初回判定 | 対応とdelta判定 |
| --- | --- | --- |
| DR-1 accountからendpointへの発見契約がない | D2/NET-INVAR/DM offlineのExisting-gap | 署名binding、QUIC identity照合、CN候補の検証、CNなしmanual/seed/DHT経路とendpoint更新を§4へ記録。設計上解消 |
| DR-2 旧DM宛先のままでは新受信routeへ届かない | D2/ADR0020のExisting-gap | message ID/暗号frame不変で輸送先だけ移行、ACK経路・重複/tombstoneを§4/5へ記録。設計上解消 |
| DR-3 非表示private channelの新epoch鍵に移れない | D2/private INVARのExisting-gap | 既知旧epochで保護した制御capsule、個別grant検証、連続rotationのcursor再開を§4へ記録。設計上解消 |

初回限定レビューはFAIL 3件、その3件へのdeltaレビューは全件解消。
未知の不具合を探す全レビューの反復は行わない。実Iroh・2端末の実証、wire値、全P2の監査は未実施。

## 実装前に残る技術確認

1. 上流の指定peer限定/neighbor受付/進行中sync停止/内部接続容量を実現するAPI。
   現行iroh-docsのstart_syncは保存peerを追加するため、既存leave/startの組合せでは不足する。
2. 受信routeのbinding・暗号capsule・private制御配送のwire contractと2端末の実証。
3. inventoryの未分類入口・sink caller・writer/GC/UIを完成し、固定ACへ対応させる。

これらは承認済み範囲の技術作業であり、ユーザーの再承認待ちではない。
局所変更は関連検証をローカル実行し、全体確認はPR/CIで行う。

今回の文書差分は `git diff --cached --check` と変更6文書のローカル参照31件を確認した。
製品コード変更がないため製品テストは実行していない。Issue本文の蓄積したCR/空行も、
非空白文字が全て同じこととGitHubへ保存された本文の一致を確認して正規化した。

## P2実証: accountとendpointの対応確認

ユーザーの自律続行指示に従い、DR-1のうち接続先を認証する契約を実装した。
新しいALPNのhandler/clientと署名型を追加するが、productionのRouterへはまだ登録しない。
既存のtopic/DM/private受信、writer、保存形式は切り替えていない。

| 固定条件 | 実装 / 証拠 |
| --- | --- |
| BIND-1: account/route/endpoint/時刻を署名で束縛し、別accountやendpointの自己申告を採用しない（NW-8/DR-1） | `ReceiveEndpointBindingV1::verify_for`、`receive_endpoint_binding_covers_every_field_with_signature`、実QUICの`receive_binding_replay_from_another_endpoint_is_rejected` |
| BIND-2: 期限/版/入力bytesを制限し、未知field・無効な時刻・過大入力を拒否する（NW-2/3） | coreの`expires_and_rejects_invalid_intervals`/`decode_is_bounded_and_strict`。wire 1,024bytes、寿命5分、未来許容1分 |
| BIND-3: CNを使わず、実endpointへ接続して対応を確認する。同一accountの複数端末を排除しない（NET-INVAR-1/NW-8） | `receive_binding_exchange_uses_authenticated_endpoint_without_cn`、coreの`allows_multiple_devices_without_changing_route` |
| BIND-4: protocol受付超過は待機せず拒否し、caller取消時に短期QUIC接続を閉じる（NW-2/4） | `receive_binding_full_server_rejects_instead_of_waiting`、`receive_binding_cancel_closes_the_connection` |
| BIND-5: binding更新でaccount/endpointを切り替えず、古い発行時刻へ戻さない（NW-7） | `ReceiveBindingProtocol::replace`、`receive_binding_replacement_cannot_switch_account_or_endpoint` |

対象入口・sinkはinventoryのN29〜31。新しい要求内容ではなく、既存NW-2/3/4/7/8のこの実装単位への対応である。
handlerの同時2件はQUIC handshake全体の上限を証明しない。共通ownerへの組込みと、通知参照の暗号化・配送・
private制御配送・旧DM outbox移行は未完了としてP2/P3へ残す。

ローカル検証:

- `cargo test -p kukuri-core --lib receive_endpoint_binding`: 5件成功。
- `cargo test -p kukuri-transport --lib receive_binding`: 5件成功。CNなしの実Iroh接続を含む。
- `cargo clippy -p kukuri-core -p kukuri-transport --all-targets -- -D warnings`: 成功。
- 全体testは実行しない。固定headの独立監査とPR/CIで全体確認を行う。

このbinding実装はPR #1306でmerge `78a7b1c2663fe1a0919d34a17ec659d767169ba9`へ統合済み。
head `9b3cb9c6`の独立監査PASS、全13CI成功、対象13pathの一致を確認した。
監査: https://github.com/kukuri-app/kukuri/pull/1306#issuecomment-5780377820

## P2実証: 暗号化した参照とprivate manifest

基準は#1306のmerge `78a7b1c2`。gossipのmessage上限4,096byteに対して、
account宛のofferを2,048byte以内にし、長いsource locatorは参照manifestへ分離した。
private manifestはさらにchannel/epoch別に暗号化する。DM frame・添付本体のサイズを縮める変更ではない。

| 固定条件 | 実装 / 証拠 |
| --- | --- |
| OFFER-1: 宛先以外の復号、senderの詐称、署名したprovider/参照/期限の改変を拒否（NW-8/9） | `seal_receive_offer/SealedReceiveOfferV1::open`。wrong-recipient、tamper、reencryption forgeryのcore tests |
| OFFER-2: wire/平文/参照manifestのbytes・版・時刻・fieldを有界にする（NW-2/3） | offer平文880byte/wire2,048byte、manifest wire65,536byte、private平文16,384byte。最大入力と無効サイズのtests |
| OFFER-3: private manifestをchannel/epoch/secretに束縛し、全epoch試行なしで照合（NW-9/DR-3） | `receive_epoch_key_id`、`PrivateReceivePayloadV1::open`。用途分離、ID連結の非曖昧性、epoch/secret違い、relabelのtests |
| OFFER-4: 一つの受信routeで四種の小さい参照を実gossipで送受信できる（NW-8/9） | `account_receive_offer_crosses_real_gossip_with_one_recipient_route`。4,096byteを超えるmanifestへの参照も2,048byte以内 |
| OFFER-5: 暗号部品はnetwork/storeを起動せず、署名済み参照をscope許可と混同しない（NET-INVAR-2） | inventory N32〜34。provider binding・scope/mutual/失効guardとblob取得/保存は後続のreceiverが所有 |
| OFFER-6: sealed offerをaccount別の単一受信routeへ渡し、切替時に旧account受信を止める（NW-7/8） | transport N60。実gossipで四種の参照、account切替/解除、送信後保持の容量を確認。復号後の認可とpayload取得は未接続 |
| OFFER-7: 署名済みprovider以外へpayload取得を拡げず、binding・byte数・hashを照合する（NW-2/8/9） | N61。provider endpoint IDとoffer期限をI/O前に固定し、そのendpointのaccount bindingを確認。共通受付内の一時取得を65,536byteに制限。wrong endpoint、binding未設置/別account、宣言長不足、保持後の期限切れ、ローカル非保存を実Irohで確認。scope/mutual/epochと永続反映は未接続 |

OFFER-6の初回監査では、receiver/送信保持taskをspawnした後に管理lockの`await`があり、caller取消で登録前taskが残る不備と、Fakeの旧account streamが切替/解除後も配送する差を検出した。前者は登録lockを先に取得してspawnと登録を非await区間に置き、停止中はhandleを台帳に残してabort完了を待つ。後者はFakeにもaccount世代の終了signalを持たせた。両方を旧実装で失敗する局所testとして固定し、修正後の関連7件が成功した。固定headの再監査とCI前にはblocker解消扱いにしない。

delta監査ではさらに、shutdownが複数のholdをdrainした後で1件ずつabort/awaitすると、途中cancelで残りの未abort taskがdetachすること、join中の送信/購読がshutdown後に再登録できることを検出した。全holdへ先にabortを発行してからawaitし、offer transportの閉鎖fenceをshutdown開始時に立てて登録直前・broadcast後も確認する。複数holdの途中cancelと送信/購読のshutdown競合を関連testへ追加した。最新headの再監査前にはblocker解消扱いにしない。

account routeは通常topicの同期診断から分離し、既存topic数・接続状態を変えない。送信の未到達peerへのjoin待ちはshutdown通知で終了する。二端末受信・各停止競合・Fakeを含む局所検証後に固定headを監査する。

関連検証はcore `receive_offer` 8件と実gossip 1件が成功。
初回compile時のfixtureのBlobHash構築と非推奨nonce変換を修正した後の結果である。
core all-targets clippyも成功。二端末の通知一覧/旧DM outbox移行の完了を、このwire往復の成功へ読み替えない。

この暗号化参照はPR #1307でmerge `cc58b367580f42c91b1097179d48f2040ffed799`へ統合済み。
head `4d96499f`の独立監査PASS、全13CI成功、対象8pathの一致を確認した。

## P2実証: 公開APIで同期futureを所有する

`Doc::start_sync`の調査だけから上流API修正が必要と判断した草案を修正した。
pin済みiroh-docsの公開`SyncHandle`と`net`のAPIで、storage actorと同期1回のfutureを分けられる。
Production nodeのdocs起動を公開`Engine::spawn`と`Docs::new`の組合せにし、同じ`DocsApi`が使用する`SyncHandle`を保持する。memory/persistentで従来のstore/authorファイルを使い、再open testで同一namespaceを確認した。これは同期先を選ぶための入口であり、既存`Doc::start_sync`とnative gossip/downloaderはまだ稼働している。受信対象・LocalOnly・private失効を維持した切替testなしに旧経路を撤去しない。

`cargo test -p kukuri-iroh-node --lib explicit_docs_sync` の実Iroh 2件が成功した。

- 保存済みuseful peerがいても、指定したpeer以外への受信callbackは0回。
- 指定peerの署名済みrecordが届き、除外peerだけにあるrecordは反映されない。
- receiverのnamespace callbackで拒否した範囲はmetadata反映0。
- 同期futureのabortで相手側のQUIC接続が終了する。
- storage actorのsyncを無効化してclose後、ローカルだけで再openしても既存recordは残る。

これは本番controllerの実装完了ではなく、NET-AC-2/3とD9のAPI実現性に対する証拠である。
同期wireや署名、保存形式の独自コピー/変更はしていない。実装移行ではnative LiveActorへ
同期を二重登録しないこと、ReplicaNoticeへの変換、全callerの停止fenceを固定する。
irohのmapped address表の回収が解消したとは扱わず、別の残確認として保持する。

## P2の前提修正: SDKの退役資源の回収

上の残確認のうち、協調peerの退役後にも資源が残る2経路を修正する。
根拠は [iroh #4447](https://github.com/n0-computer/iroh/pull/4447) と
[gossip #162](https://github.com/n0-computer/iroh-gossip/pull/162)（#161を含む）。
どちらも採用時点で未mergeであり、完全SHAへ固定する。

| 条件 | 変更前 | 候補での確認 |
| --- | --- | --- |
| RESOURCE-1: 終了したpeerのmappingを回収し、active peerと再接続を保つ（NET-AC-2(f)、D9） | registry iroh `f2eb930d`で100退役＋1 activeが101件残り失敗 | `adf5b0e0`で履歴100/1,000とも64件、active mapping保持、relay逆引き回収、再接続mapping更新。既存のactor再起動競合testも成功 |
| RESOURCE-2: 両topic lease終了後にQUICを終了する（NET-AC-2(e)、NET-TR-3） | registry相当gossip `2ce78afe`のweak close観測が12秒で失敗 | `c42f40a1`は約5秒でclose。endpoint全体を先に終了せず確認 |
| RESOURCE-3: 別topicと同時dialを壊さず、pending joinのquit後に不要なretryをしない（NET-AC-3、NET-TR-2/3） | 新しい回帰条件として固定 | 別topic継続、pending join終了と新しい需要での再joinの2件成功 |
| RESOURCE-4: 既存の停止・Direct P2P優先・relay回復を保つ（LIFE、NET-INVAR-1/2） | P1等の既存契約 | 選択peer/取消2件、docs lifecycle4件、到達不能home relay中のdirect接続1件、relay受信停止後の回復1件成功 |

最初の試験は`remote_info`のpath使用状態を接続生存と取り違えていた。候補でもその値は残るため、
その失敗を接続の証拠には使わず、connectionを保持しないweak close通知に直した。
その同じtestで旧gossipの失敗と候補の成功を確認した。docs lifecycleの最初のfilterも0件だったため、
成功件数へ含めず、`iroh_sync::lifecycle::tests`で4件の実行を確認した。

独立レビューは元registry commitから候補までを確認し、新たな権限拡張、wire/永続形式の破壊、
active peerの無条件切断を認めなかった。iroh 1.0.3、gossip 0.101.0、MSRV 1.91を維持する。
一方、actor全体の上限、relay mapのretain走査、未完結stream/header、pending join容量は未達であり、
この修正をD9/NET全体の完了へ拡張しない。元の利用者環境のCPU急増を再現・解決したとの扱いでもない。

検証入口:

- `python tools/check_iroh_resource_contract.py --revision f2eb930dda3779c6d852b72f3712aacd6e573ab1`: 変更前の失敗。
- `python tools/check_iroh_resource_contract.py`: 固定candidateの回収contractとactor再起動競合の2件。
- `cargo test -p kukuri-transport --lib connection_release`: lease終了、別topic継続、pending joinの3件。
- `cargo test -p kukuri-iroh-node --lib explicit_docs_sync`: 2件。
- `cargo test -p kukuri-docs-sync --lib iroh_sync::lifecycle::tests`: 4件。
- `cargo tree --locked -i iroh --depth 1`をroot/Tauri両workspaceで確認。lock差分は5packageのsource/checksumのみ。

全体はPR/CIで確認する。固定headの独立監査とmerge照合を終えるまでは、この段階を完了としない。

## P2/P3の受付管理: 実装する有限範囲

基準 `cc58b367`。NW-1〜4/7、NET-AC-2の受付部分として、`transport::work_admission`にI/Oを持たない状態機械を置く。現在のRemoteFetchRetryState/remote_fetch::run_single_flightは、permit取得前にtaskと台帳を増やす。この入口を共通ownerへ移す前に、受付・実行選択・取消の契約を固定する。

- ADMIT-1: active scope 64、要求256、待機者64/要求、metadata payload計4MiB、実行8を同時に制限し、満杯は型付きDeferred/Deniedで返す。受付はspawnしない。
- ADMIT-2: 同じscope世代/object/protocol/mode/persistence/byte limit/deadlineだけ合流する。同一要求の更新はI/O選択を増やさない。
- ADMIT-3: 4:2:1のlane巡回、待機時間込みdeadline、期限切れのI/O開始0。deadline索引で回収し、履歴全件のsort/retainをしない。
- ADMIT-4: 最終表示待機者の取消・scope失効は実行停止要求を出す。通常取得の待機者取消は実行を保持する。停止完了まで実行枠を解放せず、遅い完了の保存許可を返さない。
- ADMIT-5: scopeの再登録・別ownerのtokenで旧要求を再利用しない。稼働対象の逆引きだけを処理し、登録/取消履歴が10倍でも台帳が増えない。

入口はscope登録/失効、要求受付/待機解除、実行選択、完了通知。sinkはこの有限なメモリ台帳と停止指示だけで、network/storeへの直接I/Oはない。権限の意味上の検証は呼出元の責務で、発行済みscope tokenの現在性を全状態遷移で確認する。実運用のI/O adapter・旧取得経路の撤去は後続であり、この状態機械単独でNET完了とはしない。

検証は上記の境界値、停止→遅延完了、再登録、10倍履歴、lane巡回の関連unit testsとtransport clippyをローカル実行する。全体はPR/CI、固定headの独立監査で確認する。

受付の関連unit testsは9件成功（`cargo test -p kukuri-transport --lib work_admission`）。transport all-targets clippyは、初回のunwrap診断4件を不変条件付きexpectへ修正した後に成功。実行していない全体suiteはPR/CIへ委譲する。これは純粋な状態機械の契約であり、停止指示から実QUICを止める結合はまだ含まない。

SDK退役資源の修正はPR #1308、merge `9dc3f051c928cfabe5ca709adb715663cf394303`で統合済み。固定head独立監査PASS、15/15 CI成功、対象16pathの一致を確認した。

## P2実証: gossipの公開topic状態機械と所有付きI/O

D9の実装前提として、公開`proto::topic::State`へ入出力を渡し、既存native Gossipと実QUICで双方向に配送するcontractを追加した。net::utilは非公開だが、topic単位のMessageとStateは公開されている。wireはtopic IDだけのpostcard stream headerと、u32 big-endian lengthで区切ったpostcard topic Message。protocol/membership本体は複製しない。

- GOSSIP-1（NET-AC-3/D9）: owner側から選択した1接続だけを使い、native peerとのjoinと双方向配送が成功する。`public_gossip_state_roundtrips_with_native_peer_on_owned_connection`。
- GOSSIP-2（NET-AC-2(e)/NW-4）: 相手がlength headerの半分だけを送りstreamを閉じなくても、所有futureの取消でQUICが終了する。endpoint全体は継続。`owned_gossip_read_cancellation_closes_even_an_incomplete_header`。

初回は上位`proto::State`が返すtopic付きMessageをそのままwireへ送り、10秒で失敗した。native wireはstream headerでtopicを束縛し、その後は`topic::Message`だけを送る。公開`topic::State`へ変更し、応答を処理してからjoin完了を待つように直した後、双方向testは1件0.09秒で成功。取消testは初回に1件成功した。失敗した実証をSDK非互換の根拠には使わない。

追加はtestと既存lock内の3packageのdev依存参照だけ。productionにはnative Gossipを維持する。topic数・timer・I/O workerの予算、逆引き、同時dial、全transport callerへの組込みは未完了。timerを動かさない短いwire往復を、長時間の資源上限の証明にしない。

公開APIの選択は、docsが`SyncHandle`+`net`、gossipが`proto::topic::State`+所有I/O。irohの`EndpointHooks::before_connect/after_handshake`と`RouterBuilder::incoming_filter`も公開され、送信前拒否・handshake後照合・受信前選別へ利用できる。hook単独には失敗/取消した接続試行を精算するAPIがないため、接続futureの所有を省略しない。SDK全体のforkやwireの独自変更を前提にせず、この構成でadapterを作る。

## 共通受信経路の前提修正: DM ACKの会話境界

N11の保存sinkを逆引きしたところ、`handle_direct_message_hint`はACKの署名・sender・recipientを確認する一方、`dm_id`をその二者から導出した会話IDと照合せず、任意の会話の送信状態更新とoutbox削除へ進んでいた。新しいaccount受信routeへ再利用する前に、D10/NW-8の保護outbox維持に必要なExisting-gapとして修正する。新しい通知種別・配送保証は追加しない。

- ACK-1: ローカル宛に正しく署名されたACKでも、相手が異なる会話のmessageを指定した場合、outbox・本文・acked_atを変更しない。
- ACK-2: 対応する会話の正しい相手からのACKは、従来通り送信済みを記録しoutboxだけを削除する。

`tests/direct_messages/ack_scope.rs::signed_ack_for_another_conversation_cannot_remove_protected_outbox`は修正前にFAILし、相手C宛outboxが相手Bの署名ACKでNoneになることを確認した。修正は署名sender/recipient照合と同じ早期return条件に、現在のlocal/peerから導出するdm_id一致を追加する。既存wire・暗号frame・message ID・DB形式は変更しない。

対象は上記helper、test、既存DMの再試行/再起動/配信の関連tests。独立監査とPR/CIで確認する。outbox全件読取りや相手別の常時購読は別の既存P3作業として残し、この修正を容量改善と扱わない。

ACK修正後の`cargo test -p kukuri-app-api --lib direct_messages`は13件成功。別会話の拒否と正しいACKに加え、初回配送、再起動後outbox、tombstone、mutual解除/復帰、既存購読状態のcontractsを含む。`cargo clippy -p kukuri-app-api --all-targets -- -D warnings`、rustfmtとdiff checkも成功。全体suiteはPR/CIへ委譲する。

## P3の最初の本番接続: 表示用取得の受付・期限・終了

`prepare_display_fetch`を共通受付状態機械へ接続する。対象は表示専用の一時blob取得であり、取得後のscope再確認と保存は従来通りapp-apiが行う。既存の通常取得のwalk枠は残し、今回の移行で同時取得数を増やさない。通常取得・docs・gossip全体の共通owner統合は後続。

- DISPLAY-1（NW-2/3）: node単位で表示要求と実行枠を制限し、待機から結果まで同じ30秒のdeadlineを使う。満杯は待機taskをspawnせず型付きエラーで返す。
- DISPLAY-2（NW-4/7）: 待機取消は後からI/Oを開始せず、表示future取消は実streamを止める。停止中の受付は復活せず、停止完了まで枠を保持する。
- DISPLAY-3（NW-7）: node終了時に受付を閉じ、待機/準備済み/実行中の表示取得へ取消を伝える。世代終了後のbytesは返さない。無関係なnodeの受付を巻き込まない。
- DISPLAY-4（既存表示contract）: `prepare_display_fetch`が成功する時点で従来のwalk枠も取得済みにし、app-apiの取得回数予算を待機だけで消費しない。通常walkとの既存同時上限を維持する。

対象pathはiroh-nodeの受付adapter/node lifecycle/remote_fetchと、既存blob-service表示取消contract。新しいguardの独立監査と、全体・slow結合testはPR/CIで行う。局所では期限前の失敗再現、新adapterのqueue/取消/終了tests、既存表示取得testsを選ぶ。

実装はnodeごとの`DisplayWorkAdmission`で、同時64個の表示leaseと8個の準備/実行枠を制御する。各leaseのmetadataはhash32byte。既存の通常walk Semaphoreも準備完了前に取得し、段階移行で通常＋表示の従来上限を増やさない。登録・取消・完了は同期的な短いlock内で台帳を更新し、I/O待機中は保持しない。表示futureを作っただけでpollしない場合も、期限/終了は実行前に確認する。

修正前の`display_admission_wait_is_included_in_total_budget`は内部の待機が終わらず外側31秒timeoutでFAIL。修正後のiroh-node `display_` 6件と、複数serviceの別retry台帳が同じnode表示枠を共有するtest1件が成功した。blob-serviceの実QUICによるcaller取消とnode終了の2件も成功。初回compileの一時hash文字列の借用を修正済み。変更2crateのall-targets clippy成功。node終了の入口でも直ちに受付を閉じる追加後、終了の実QUIC testを再確認する。

全体とapp-api slow/実scenarioはPR作成後にCIを使う。受付9件/gossip実証2件の既存結果は#1309（merge `81dcff79`、独立監査PASS、15/15 CI、10path一致）として再利用する。

node終了入口の取消を追加後の`node_shutdown_cancels_display_fetch_without_returning_or_caching_bytes`は1件成功（0.11秒）。再現/取消/別node/複数serviceの証跡は上記の結果を採用し、無関係なsuiteを再実行しない。

## P3の通常取得統合: 実装する有限範囲

表示と通常取得の受付を同じnode所有台帳へ統合する。既存の`run_single_flight`は予約後にspawnし、そのtaskがSemaphoreを待つ。新しい通常取得は有限queueへfutureを保持し、共通実行枠を得たものだけ起動する。合流は従来のservice/retry台帳・flight key単位を維持し、private/公開や一時/保存を新しく混ぜない。

- FETCH-1（NW-1/2/3）: 表示＋通常取得をnode全体8実行に制限。queue/metadata/waiterは上限内、待機込み期限で、満杯時に待機taskをspawnしない。
- FETCH-2（既存#1207/NW-4）: 同じ通常取得は1回だけ実行し結果を共有。呼出元cancel後も開始済みworkの結果・失敗cooldownを所有し、queuedの最後の待機者cancelは後からI/Oを開始しない。
- FETCH-3（NW-7）: node終了で待機/実行/結果を停止し、panic・期限切れ・取消でも台帳と枠を回収。別nodeは維持。
- FETCH-4（NW-2/3、D8）: cooldown台帳を期限索引と1,024件上限へ移し、全件retainを除く。停止・回復・保存方針を台帳の整理で変更しない。

対象はiroh-nodeの共通runtime/remote_fetch、transportのretry台帳、関連tests。既存のlocal fast pathとapp-api保存guardは維持。全protocolのscope/peer選択・docs/gossipの常時同期廃止は別の未完了作業であり、今回のnode単位取得枠統合へ混同しない。

通常取得のqueueは`NetworkWorkRuntime`へ統合した。表示adapterを移動/拡張し、既存の合流・通常caller取消後の結果所有を維持する。最初の受付から30秒、64scope/1flight64waiters/表示＋通常8実行。callbackがretry結果を記録してからidentityを退役し、その間は新要求も同じ結果へ合流する。retry guardの読取りから同期的な受付までguardを保持し、完了との競合でcooldownを迂回しない。

修正前の`concurrent_walks_are_bounded`は、待機中に取消された3件が後から開始し11回となってFAIL（新しいNW-4の期待は開始済み8回だけ）。この条件を明示的に更新した後、既存remote_fetch12件が成功した。開始済み通常取得のcaller取消後のcooldown/結果共有は既存testを維持した。失敗cacheも10,240件が残るFAILを確認後、期限索引と上限1,024件へ修正し、関連transport4件が成功。

runtimeの表示＋通常の容量共有、waiter上限と合流期限、panic完了、queued future破棄時のNode Dropによる再入、node終了の8件が成功。queue futureを受付lock内でdropするとNode::dropのcloseで同じlockへ再入し得るため、退役したentryをlock外へ返してから破棄する。診断ラベルも保持bytesを制限し、本文/添付の有効サイズを縮めない。

増えたremote_fetchのtest moduleは同名の別ファイルへ移し、test名とscopeを保持した。1,000行のratchetをbaseline増加で回避せず、関連testの分離だけを行う。通常取得のlane/意味上のprivate世代やSDK内部の取得全体の統合は残作業であり、今回の受付・成否cacheの有限化に含めない。

最終の局所検証: remote_fetch12件、共通runtime8件、blob-serviceの実取得/一時取得/取消等7件、transport retry4件が成功。追加のUTF-8診断ラベル上限1件と、cooldown key byte上限を含む容量test1件も成功。変更3crateのall-targets clippy、rustfmt、diff、サイズ検査成功。全体とapp-api slow/実scenarioはPR/CIへ委譲する。

### 固定head監査で見つかったservice世代の修正（B6）

head `0d4506ad`の独立監査はN47/FETCH-2でFAIL。service IDにretry Arcのアドレスを使うと、完了callbackが最後のArcをdropしてからidentityを退役するまで、旧identityが残ったままそのアドレスを再利用できる。callback/queueがArcを保持するという最初の説明は、この最後の窓を覆っていなかった。

修正前の所有権順序を監査で確認し、allocatorの再利用時機を成功条件にする不安定なtestは置かず、旧serviceをdropしてidentity退役前で止めるcontractを追加した。retry台帳の構築時に単調な非再利用IDを割当て、取得時はそのIDを使う。`retired_service_identity_cannot_be_reused_before_old_flight_is_removed`は旧serviceをcallbackで解放し、同じkeyの新serviceが別flightになって別の結果を得ることを確認して1件成功。cooldown記録→identity退役の順序は維持する。修正deltaの独立監査を別途行う。

## P3: blob protocolの成否台帳と有界なpeer候補

共有対象はblob protocolの成否・接続観測・要求頻度。learned/seed/importedの入口台帳はservice別に維持する。単純に入口台帳も共有すると、blob側の先行learnでdocs側の変更boolがfalseになり、既存reapplyを省略するためである。gossip/docs同期の観測をblob成功へ混ぜない。

- PEER-1（NW-6/D6）: 型付きNotFound/部分欠損を接続不良と区別する。現SDKが欠損にも返すERR_INTERNAL(3)は本当の内部エラーと区別不能なので「拒否/内部結果」として別計数し、NotFoundと断定しない。接続不良はbackoffし、ローカル保存失敗はpeerへ転嫁しない。
- PEER-2（NET-AC-2/3）: 同じnodeのdocs/blob helperはblob成否/要求頻度を共有し、入口台帳の変更通知・既存のsource優先度を維持する。
- PEER-3（NET-AC-2）: 成否cacheとP2P要求頻度のsubjectを各1,024件に制限する。稼働attemptはRAIIで保持し、古い完了が新しい世代を上書きしない。頻度窓の途中で記録を捨てて制限を迂回させない。
- PEER-4（NET-AC-2/3）: fetchの候補はlearned/seed/importedをそれぞれ最大4件のcursorで読み、最大12候補だけ順位付けし最大4peerを選ぶ。全historyのclone/sortを行わず、次回は窓を進める。source自体の保持とnative同期/診断の全件APIは後続。

変更はtransportのpeer部品、iroh-node composition/remote fetch、docs-sync/blob-serviceのconstructorと関連tests。実NotFoundの修正前testは失敗を5件計上してFAILした。candidate読取り量とcache/要求頻度を境界testで固定し、既存connect候補順序と実blob往復をローカルで確認する。全体/slowはPR/CI、固定headは独立監査する。

### blob protocolのpeer観測と候補窓の局所結果

`PeerAddrBook`のlearned/seed/importedはdocs/blobそれぞれのままにし、node所有`BlobPeerHealth`だけを共有した。入口台帳まで統合するとblob側の先行learn後にdocs側の変更boolがfalseとなりreapplyを失うため。healthはblob ALPNの接続/検証済み転送の観測・要求頻度だけを持ち、gossip/docs同期の成功として流用しない。

- 実Irohの空providerへStore/Ephemeral/Boundedの3種類で問合せると、修正前は5件の転送失敗を記録した。pin済みiroh-blobs 0.103.0はこの欠損もstream reset `ERR_INTERNAL(3)`で返すため、欠損と確定できない。型付き欠損は`fetch_misses`、3等のstream応答は`fetch_rejections`へ分離し、どちらも接続不良のbackoffを増やさない。ローカルblob store故障はpeer失敗にせず呼出元へエラーで返す。両実Iroh testsが成功。
- 1nodeの成否cache1,024件とP2P要求頻度subject1,024件を別に上限化した。接続attemptが使う観測をArcでpinし、古い世代の遅い終了は新しい接続判定を上書きしない。生存する要求頻度窓は容量超過時に捨てず、新しいsubjectを延期する。履歴2,048件、全1,024件pin、頻度窓の関連testsが成功。
- 取得候補は各sourceでcursorを進めながら最大4 IDだけ読み、直近に成功した2 IDと30秒以内に得た4 IDを同じsourceに存在する場合だけ加える。実addressを最大12件だけmaterialize/rankし、1要求は最大4peerへ進む。新しいmanual ticketが既存の成功peer4件に押し出される失敗を修正前に再現し、最新ticketを4枠に含める。直近ticketの連続blob要求も修正前FAIL→修正後PASS。source履歴100/1,000件で読取りは各source4以下、別bookにだけ存在する健康peerは選択0。transport関連testsと実blob取得・docsのdirect候補testが成功。

保存済みsource台帳とSDK内部address mapはまだ上限化していない。`merged_peers`、`available_peer_ids`、保存/復旧のsnapshotは全sourceを読むのでP3に残し、この結果を全peer経路の完了とは判定しない。protocolごとのroute優先度はper-peer `connect_candidates`のdirect→remote_info→relay順を維持した。全体・slow/複数nodeの確認はPR/CIへ委譲する。

## P3: 本番nodeへの署名付き受信binding登録

ADR 0055 §4.1の交換proofを本番nodeで利用できるようにする。対象はN53〜55とNW-8のendpoint対応であり、
暗号化offer配送、scope認可、既存通知/DM listenerの移行は後続である。

- node生成時はアカウント鍵が未読込なので、Routerには空のslotだけを登録し、要求を即拒否する。
- identity load後のruntimeだけが同じnodeへ鍵を導入する。別accountへの差し替えは拒否し、
  stack再構築では旧nodeを停止してから同じ鍵を新nodeへ導入し、service差し替え前に応答可能にする。
- 期限切れを避ける独立timerを増やさず、2件のserver受付の中で要求時に最大300秒のbindingを署名する。
  shutdown開始時は新規受付を閉じ、進行中要求の終了を待って鍵を解除する。

局所確認はtransportの実QUIC slot testとdesktop-runtimeの実node再構築test、変更crateの
関連clippy・サイズ検査を行う。全体・slowはPR/CI、固定headの鍵/外部応答境界は独立監査で確認する。

#1313 merge後のmain `6b03893a`へ差分を載せ直し、nodeの共有healthと受信slotを併存させた。
局所結果はtransportのbinding交換6件、desktop-runtimeの実account起動/stack再構築2件、
移動した既存docs author test1件が成功。変更3crateのall-targets clippy、rustfmt、差分、
ファイルサイズ検査が成功。全体・slowは同headのPR/CIへ委譲する。

## P3: N16のOS通知dispatchを新規rowだけのcursorへ移す

現行のTauri `poll_once`は受信eventと60秒fallbackのたびに`list_notifications`で全履歴を読み、
最新`received_at`の全IDをcursor JSONへ保存する。2,048件が同一時刻ならcursorの件数とサイズも
2,048件分になる。storeのSQLite queryも全rowを`fetch_all`する。これをN56/57として分離し、
公開/非表示通知の既存対象範囲、OS設定、成人向けpreview guard、local-only inboxを維持する。

- NOTIFY-1: 新規INSERTにだけ単調sequenceを割り当て、既存rowはmigration時にNULLのまま保持する。
  重複INSERTはsequenceを増やさず、SQLite索引とmemory indexで64件ずつ読める。
- NOTIFY-2: 初回起動・account切替・restoreはheadをbaselineにし、旧通知を一斉にtoastしない。
  同じ時刻の新通知でも挿入順に漏れなく進み、cursorの保存サイズは履歴件数に依存しない。
- NOTIFY-3: 1ページごとにaccount切替guardを解放し、各通知のquiet/read/self/種類設定と
  成人向けpreview gateをOS表示より先に適用する。手動のinbox一覧は既存UI契約のまま残す。

修正前のsource確認では`list_notifications().fetch_all`、`compute_cursor`の同時刻ID全列挙を確認。
この境界のTauri回帰testを先行追加したが、Windowsローカルのtest executableは
`STATUS_ENTRYPOINT_NOT_FOUND`で起動前に停止したためFAIL証跡には採用しない。
同testはWSL/Linuxで実行し、storeの挿入順/移行負例と関連crateのcompileも局所で確認する。

実装では新規INSERTだけを単調sequenceへ登録するSQLite triggerと、同じ順序を保つmemory indexを
追加した。既存通知rowはNULLのままで、重複INSERTは番号を消費しない。64件のrange queryには
`idx_notifications_dispatch_seq`を使うことをquery planで確認。Tauriはページ後にguardを解放して
続きだけを処理し、cursorは単一整数にした。旧cursorの大きなファイルは1,024byteで読取りを止めて
baselineへ戻す。旧OS設定・成人向けpreview・self/read判定はOS表示前のまま維持した。
baselineの永続化に失敗した場合はbaseline_pendingを維持し、次のpollで再試行する。

局所結果: storeの新規/重複/同時刻129件の3ページと再起動、旧rowを再toastしないmigrationの2件、
既存notification backend parity、app-api通知14件、31世代のup/down/replayとschema goldenが成功。
変更したstore/app-api/desktop-runtimeのall-targets clippy、Tauriのcheckとtest binary compileが成功。
WindowsのTauri test executableは`STATUS_ENTRYPOINT_NOT_FOUND`で起動できなかったが、
WSL/Linuxで当該`background_notifications` 8件が成功した。
Tauri clippyは既存の3種類のlint（`drop_non_drop`、`collapsible_if`、`err_expect`）だけを
明示的に除外して成功した。全体とslowはPR/CIで確認する。

初回PR headの全体Rust CIで、別のmigration契約`migrations.rs`が世代数を30に固定していてFAILした。
新世代のup/downが揃う判定と全replay後の件数を31へ更新し、関連migration 12件を局所で確認した。
`migrations_roundtrip.rs`の31世代/goldenと合わせ、固定値の両入口を更新してからCIを再実行する。

## P3: N25の通知event待機taskをaccount runtimeへ所有させる

`DesktopRuntime::new`は`notification_inserted_notify`を待つtaskをspawnし、handleを保持しない。
account shutdown後にも旧Notifyとbroadcast senderをtaskが保持するため、旧accountの通知signalを
送る経路とtaskが残る。N58としてtaskの所有・停止だけを修正し、通知rowの保存/OS toastや
host側のdesired購読復元の意味を変えない。

- EVENT-1: account runtimeあたり転送taskは1件で、通知挿入時は従来どおりeventを1件転送する。
- EVENT-2: shutdown開始時にtaskをabortし終了を待つ。終了後の旧Notifyではbroadcastしない。
- EVENT-3: 明示shutdownを経ないruntime Dropでもtaskをabortし、旧accountの待機taskを残さない。

修正前の`notification_event_forwarder_stops_when_runtime_shuts_down`は旧Notifyを再通知すると
shutdown後にもeventを受け取ってFAIL。task handleをruntimeへ保持してshutdownでabort/await、
Dropでもabortする修正後に同testとDrop testが成功した。既存sync observer/wireと合わせた
runtime event4件、共有資源lock分類1件が成功。変更crateのclippy/format/sizeを局所で確認し、
全体とslowはPR/CI、固定headのaccount境界は独立監査に委ねる。

## P3: N28のCN topic rendezvous候補を期限つき窓から返す

現行`TopicRendezvousStore::heartbeat`はauth/consent後の各topicで`SMEMBERS`→全ID sort→
各peerのGETを行う。topic参加者が65件のときrequesterに65候補を返すFAILをValkey実接続で再現。
topic keyは公開/privateとも既存のopaque hashを使い、CN応答自体をaccount証明にはしない。

- RENDEZVOUS-1: 15秒bucketの直近4窓から各16件だけ候補を標本し、最大64件のmembership/peerを
  調べて最大8件返す。Redisの正count`SRANDMEMBER`はcountに比例し、総集合は読み出さない。
- RENDEZVOUS-2: topic-peer所属とpeer metadataは45秒TTL、bucket keyは60秒TTL。
  時刻はValkey `TIME`で共有し、複数CN API host間の時計差で別窓へ分かれない。
  別topicで同じpeerのmetadataが更新されても、失効した旧topic所属を復活させない。
  leaveは直近4窓と所属keyを除去する。旧SET keyは45秒の既存TTLで自然退役する。
- RENDEZVOUS-3: `cn-user-api`のbearer endpoint/consent guard、opaque topic key、
  relay URL付与とレスポンスwireを維持し、CN無しのP2P経路は変更しない。

修正後は65参加者で8候補、cross-topic期限/leaveと窓/member TTLのValkey関連3件が成功。
Postgres+Valkeyを使う既存CN APIのfresh candidateとprivacyの2件も、integration flagを
有効にして成功した。無効topic入力がValkey接続より先に拒否される負例1件も成功。
全体・slowはPR/CI、固定headの認証/同意/秘密境界は独立監査に委ねる。

固定head監査ではSADD成功後に別commandのEXPIREへ進む途中でcancel/通信失敗すると、
新bucketがTTLなしで残るblockerを発見した。SADDとEXPIREをRedis pipelineのatomic transactionへ
統合し、`MULTI→SADD→EXPIRE→EXEC`を固定するcontractと正常時のTTL testで確認する。
旧headの監査FAILはこのdeltaがPASSするまでmerge根拠に使わない。

修正headのCN CIでは新Valkey実接続test3件が同時に`timed out`でFAILした。composeのValkeyは
healthyで、失敗は約500ms後。pin済みredis clientの既定response timeoutは500msで、
局所の単独実行では3件とも成功していた。rendezvous専用接続のconnect/response timeoutを
2秒に明示し、無制限待機や再試行taskを増やさず、負荷時の関連testを再確認する。
