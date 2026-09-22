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

関連検証はcore `receive_offer` 8件と実gossip 1件が成功。
初回compile時のfixtureのBlobHash構築と非推奨nonce変換を修正した後の結果である。
core all-targets clippyも成功。二端末の通知一覧/旧DM outbox移行の完了を、このwire往復の成功へ読み替えない。

この暗号化参照はPR #1307でmerge `cc58b367580f42c91b1097179d48f2040ffed799`へ統合済み。
head `4d96499f`の独立監査PASS、全13CI成功、対象8pathの一致を確認した。

## P2実証: 公開APIで同期futureを所有する

`Doc::start_sync`の調査だけから上流API修正が必要と判断した草案を修正した。
pin済みiroh-docsの公開`SyncHandle`と`net`のAPIで、storage actorと同期1回のfutureを分けられる。

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

## 共通受信経路の前提修正: DM ACKの会話境界

N11の保存sinkを逆引きしたところ、`handle_direct_message_hint`はACKの署名・sender・recipientを確認する一方、`dm_id`をその二者から導出した会話IDと照合せず、任意の会話の送信状態更新とoutbox削除へ進んでいた。新しいaccount受信routeへ再利用する前に、D10/NW-8の保護outbox維持に必要なExisting-gapとして修正する。新しい通知種別・配送保証は追加しない。

- ACK-1: ローカル宛に正しく署名されたACKでも、相手が異なる会話のmessageを指定した場合、outbox・本文・acked_atを変更しない。
- ACK-2: 対応する会話の正しい相手からのACKは、従来通り送信済みを記録しoutboxだけを削除する。

`tests/direct_messages/ack_scope.rs::signed_ack_for_another_conversation_cannot_remove_protected_outbox`は修正前にFAILし、相手C宛outboxが相手Bの署名ACKでNoneになることを確認した。修正は署名sender/recipient照合と同じ早期return条件に、現在のlocal/peerから導出するdm_id一致を追加する。既存wire・暗号frame・message ID・DB形式は変更しない。

対象は上記helper、test、既存DMの再試行/再起動/配信の関連tests。独立監査とPR/CIで確認する。outbox全件読取りや相手別の常時購読は別の既存P3作業として残し、この修正を容量改善と扱わない。

ACK修正後の`cargo test -p kukuri-app-api --lib direct_messages`は13件成功。別会話の拒否と正しいACKに加え、初回配送、再起動後outbox、tombstone、mutual解除/復帰、既存購読状態のcontractsを含む。`cargo clippy -p kukuri-app-api --all-targets -- -D warnings`、rustfmtとdiff checkも成功。全体suiteはPR/CIへ委譲する。
