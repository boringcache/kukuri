# REFACTORING.md

[AGENTS.md](AGENTS.md)の作業原則・設計原則を前提とし、受入条件を満たす最小の最終コード量を目指す。起票・計画・PR・監査・Closeは `docs/runbooks/issue-lifecycle.md` に従い、本書は構造整理と検証の選び方を具体化する。

この文書は、AIエージェントと人間が kukuri でリファクタリング作業を行うときのルールを定義する。

この文書の構造調査・互換性境界・成果物の規定はリファクタリングに適用する。「path別検証マトリクス」と「検証の選定・中断」は他種別の変更でも使う。文書だけの修正にコードの構造調査や分割計画を要求しない。

## リファクタリングモード

純粋なリファクタリングでは合意した外部挙動を維持する。機能追加・修正・移行に必要な共通化、呼出元切替、旧処理削除は、その目的の一貫した変更としてまとめてよい。

リファクタリングとは、外部挙動を維持したまま、内部構造・命名・責務境界・重複・ファイル構成を改善する作業である。

外部挙動を変更する場合は、それは refactor ではなく feature / fix / migration として扱う。

## 非目的

- リファクタリング作業を使ってプロダクト挙動を追加しない。
- タスクで明示されない限り、CI の必須 / 任意 job 設定を変更しない。
- ついでの整理として crate 分割や大型ファイル分割を行わない。

## 基本原則

- 1 PR = 1意図。
- 一つの成果に必要なrename / move / extractionと処理の置換・削除をまとめ、機械的な移動と責務変更を追えるようにする。独立した目的だけを別PRにし、差分の小ささを理由に二重経路を残さない。
- public API、protocol、storage、正本等を変更する場合は、合意した目的と利用条件に必要かを確認し、移行・互換性の条件を固定する。承認済み範囲の実装方法は担当者が判断し、未承認の製品契約変更だけをユーザーへ確認する。
- 重複・未使用・廃止経路のtestは統合・削除し、受入条件に必要な検証が残ることを示す。必要な検証の削除・弱体化によって成功扱いにしない。
- 振る舞いが変わる可能性がある場合は、先に characterization test / contract / scenario を追加する。
- 大きな抽象化を導入する前に、現在の責務境界と呼び出し方向を調査する。
- 同じ状態・判断の重複は共通の責務へ集約する。所有と依存を明示し、将来用途だけの抽象化や同じ検証の個別追加で総量を増やさない。

## リファクタリングで達成すること

合意した外部挙動を満たす候補の中で、完成後に保守する実装・テスト・補助コードの総行数を最小にする。
見かけの圧縮・移動を削減として数えず、置換・統合・削除によって理解・変更箇所・波及範囲を小さくする。
候補ごとに、次のうち何を改善するかを具体的に示す。

- 責務の所有者と依存方向を明確にする。
- 同じ状態、判断、変換の正本を一つにする。
- 必要以上に広い公開面、境界横断、暗黙の結合を縮小する。
- 同一責務内で意味が同じ重複を統合する。
- I/O、時刻、乱数、network、永続化などの副作用を、挙動を検証できる境界へ隔離する。
- 到達不能な処理、未参照経路、sunset 条件を満たした互換経路を、入口・参照・契約を確認して除去する。
- 名前、型、配置、文書を現在の責務とリポジトリの現行規約へ一致させる。

次は候補発見の signal にはなり得るが、単独では着手理由にしない。

- 見た目や記述様式が好みと異なること。
- 行数、ファイル数、複雑度、coverage など一つの数値だけが閾値を超えたこと。
- 一般的な best practice と異なっていても、kukuri で変更摩擦や回帰リスクが観測されていないこと。
- 将来必要になるかもしれない抽象化を先回りして作ること。
- 変更予定も圧力もない安定領域を、全体統一のためだけに書き換えること。

監査の正当な結論として「現時点では変更しない」を認める。観測した問題、構造上の成果、挙動維持の
証拠を具体化できない候補は着手しない。

## 定期監査と個別実行

定期監査は原則として product code を変更せず、有限の範囲で候補を収集し、根拠を確認して分類する。
監査結果をそのまま一つの大型リファクタリング PR にしてはならない。実施する候補は、一つの構造上の
成果を持ち、単独で検証・review・差し戻しできる個別 Issue / PR として扱う。

### 監査を開始する trigger

固定した週次・月次で repository 全体を掃除するのではなく、次のいずれかを観測したときに開始する。

- 大規模な feature または milestone が完了した。
- release の安定化へ入る前である。
- 次の変更を現構造が明確に妨げている。
- 同種の不具合、review 指摘、変更漏れ、変更時の摩擦が繰り返された。
- 複数の実装が重なった領域で、局所的な別実装や二重 state が増えた。
- `cargo xtask oversized-files` など既存 ratchet の基準値増加や、責務境界からの逸脱を観測した。
- 互換経路、feature flag、一時実装の明示済み sunset 条件を満たした。

### 監査の有限化と成果物

開始前に、一つの subsystem、直近の変更領域、または具体的な責務境界へ scope を限定し、時間枠または
確認する入口・symbol・path の集合を固定する。成果物は、根拠付き候補一覧、優先順位、各候補の分類と
理由、今回着手する候補とする。子項目は同じIssueで管理でき、個別Issueの作成を終了条件にしない。

各候補は次のいずれかへ分類する。

- `実施`: 固定した受入条件と開始条件を満たし、対応する作業へ進める。
- `延期`: 明示依頼された範囲だが、前提が未充足である。未依頼のエッジケースは対象外とし、自動着手しない。
- `却下`: 構造上の成果を示せないか、変更コスト・回帰リスクが価値を上回る。
- `別種別`: 挙動不良、新機能、依存更新、protocol / storage 変更、性能改善、文書だけの不一致として
  `fix` / `feature` / `deps` / `migration` / `docs` などへ分離する。

固定 scope の候補をすべて分類した時点、または事前に決めた時間枠を使い切った時点で監査を終了する。
PR が一つも発生しなくても正常な完了とする。

### 候補に必要な根拠

| 観点 | 確認対象 | 着手判断に必要な根拠 |
|---|---|---|
| 不要物・互換経路 | dead code、未参照 export、古い flag、一時経路、古い comment | 参照検索、runtime の入口、sunset 条件、関連 test / contract。serde / IPC / macro / config / 動的登録 / generated code も確認する |
| 重複・正本 | domain rule、validation、key 生成、state 更新、projection の二重実装 | 各実装の利用者と責務所有者、意味が同一である証拠、変更漏れや摩擦の実例 |
| 責務・依存 | 複数責務を持つ module、境界横断、循環依存、広い公開面 | 現在と目標の責務・依存、変更波及の実例 |
| 制御・副作用・検証可能性 | 深い分岐、隠れた state、順序依存、副作用との混在 | 維持する transaction / retry / cancel / lifecycle / concurrency / P2P 順序、error semantics、保護する test / scenario |
| 現行規約とのずれ | 共通機構を使わない局所実装、古い実装様式、命名・型・配置のばらつき | repository 内の正本と現行の模範実装。一般論だけを根拠にしない |
| 永続的な知識 | 実装と ADR / runbook / comment の不一致、検証手順の欠落 | 現行実装、test、repository 内文書。session-local な資料へ依存しない |

### 選定、開始、停止

優先順位は、変更頻度、今後の具体的な変更予定、開発・review の摩擦、不具合や互換性への波及、
検証可能性、実施コスト、差し戻しやすさを合わせて判断する。数値は候補発見の signal として使い、
数値だけで自動選定しない。

個別作業は、次をすべて説明できる場合だけ開始する。

- 観測した問題と、それを示す path、symbol、参照、履歴、test などがある。
- 現在の責務、依存、外部挙動、互換性境界、互換パスを特定している。
- 目標とする構造上の成果と、その before / after の成功判定がある。
- 維持する挙動と、それを証明する最小の validation がある。
- 対象と対象外が固定され、一つの意図として review・検証・差し戻しできる。

次のいずれかで固定した完了範囲を変える必要が判明したら、該当作業を止め、範囲を先に再定義する。承認済みの変更として計画に含まれるAPI・保存形式等の変更に再承認を要求しない。

- 現在の挙動または正本を特定できない。
- 互換性境界、protocol、storage、public API、利用者に見える挙動の変更が必要になった。
- validation で挙動維持を十分に証明できない。
- 目的が `fix` / `feature` / `deps` / `migration` / 性能最適化へ変わった、または required CI の変更が必要になった。
- 当初の構造改善が不要と分かったか、具体的な成果を示せなくなった。
- 一つの意図として review・差し戻しできる範囲を超えた。

### 個別作業の成果物ベースの完了条件

個別作業は次をすべて満たしたときに完了とする。

- 観測した問題と、実施前後の構造上の変化が path、symbol、依存、責務などの証拠へ対応している。
- 状態の正本数、重複経路、依存、公開 symbol、境界横断、未参照経路など、目的に合う指標が改善している。
- public API、protocol、storage、config、event、UI など対象領域の外部挙動が維持されている。
- 変更前後の必要な validation が記録され、既知の失敗を除いて新しい failure / warning がない。
- test / contract / scenario の削除、skip、弱体化で成功させていない。
- code の移動や分割だけでなく、定義した責務、依存、正本の問題が実際に改善している。
- 必要な ADR、runbook、comment、本書が現行実装と一致している。
- PR が一つの意図に限定され、単独で review・差し戻しできる。
- 未依頼のエッジケースと対象外の改善を追加しておらず、必要性を受入条件で説明できない未使用基盤・新旧併存が完成形に残っていない。

「行数が減った」「ファイルを分割した」「coverage が上がった」「AI が成功と報告した」だけでは
完了としない。

## PR種別

PRタイトルまたはタスク概要では、以下のラベル / prefix のいずれかを使う。

- `refactor:rename`: 名前変更のみ。ロジック変更禁止。
- `refactor:move`: ファイル移動のみ。ロジック変更禁止。
- `refactor:extract`: 関数・型・モジュール抽出。外部挙動変更禁止。
- `refactor:boundary`: crate / module 境界整理。先に計画を書く。
- `refactor:delete`: dead code 削除。参照経路調査を添える。
- `contract`: 仕様固定テスト追加。実装変更禁止。
- `scenario`: harness scenario 追加/更新。プロダクト実装変更は別PR。
- `fix`: バグ修正。修正前の証拠は `docs/runbooks/issue-lifecycle.md` の「修正前の再現」に従う。
- `deps`: 依存更新。リファクタリングと混ぜない。
- `docs`: ドキュメント更新。実装変更と混ぜる場合は理由を書く。

PR を作成または説明するときは、タスク種別を明確にする。リファクタリングPRに機能追加や挙動変更を隠さない。

推奨 PRタイトル prefix:

```text
[codex][refactor:rename] ...
[codex][refactor:extract] ...
[codex][refactor:boundary] ...
[codex][refactor:delete] ...
[codex][contract] ...
[codex][scenario] ...
[codex][fix] ...
[codex][deps] ...
[codex][docs] ...
```

PR共通欄は `.github/PULL_REQUEST_TEMPLATE.md` を使い、本書の「記録テンプレート」にある構造上の成果・挙動維持の証拠へリンクする。テンプレートを別々に複製しない。

## PRの境界

一つの受入結果に必要な実装・共通化・test・呼出元切替・旧経路削除をまとめる。独立した目的の変更は混ぜない。
移動や整形だけの差分と挙動変更をreview上で区別できるようにするが、変更種別の組合せだけで別PRを強制しない。

## 互換性境界

以下は変更時に互換性を確認する対象である。合意した目的に必要な変更は、該当する移行条件を先に固定する。
既存形式の維持自体を目的にせず、必要な互換性と最小の最終実装を両立する。contractの失敗は、
合意した挙動への影響と照合し、意図した契約変更と回帰を区別する。
判断に必要な根拠と検出 test は本節へ集約し、repository 外や非追跡の計画を前提にしない。

1. **署名 canonical 3 系統**: envelope の 6 要素配列(`crates/core/src/envelope.rs` の
   `canonical_envelope_payload`)、DM frame/ack の canonical 配列(`crates/core/src/direct_messages.rs`)、
   moderation event のキー辞書順 canonical(`crates/cn-safety/src/event.rs`)。
   固定: `crates/core/src/tests/signing_canonical.rs` / `crates/cn-safety/tests/domain_model.rs` /
   `crates/cn-safety-runtime/tests/signer.rs`。
2. **rendezvous / 派生文字列**: replica 命名と `kukuri-docs:` 派生(`crates/docs-sync/src/replicas.rs`)、
   gossip topic id(`crates/transport/src/iroh/topics.rs` の `topic_to_gossip_id`)、
   rendezvous ドメイン(`crates/core/src/rendezvous.rs`)、DM の HKDF/AAD ドメイン群、
   wire prefix 定数(`crates/core/src/wire.rs`)。
   固定: `crates/core/src/tests/derivation_golden.rs` / `wire_constants.rs`、
   `crates/docs-sync/src/tests/replicas.rs`、transport の `topic_to_gossip_id_matches_golden`。
3. **serde フィールド名・variant 名 = wire 形式**: `GossipHint`(externally-tagged、受信側は
   デコード失敗を黙殺)、docs エントリ値の `KukuriEnvelope` / `*DocV1` 群、envelope kind/tag 文字列。
   固定: `crates/core/src/tests/wire_snapshot.rs`。
4. **`PayloadRef` の PascalCase タグ**: 周囲の snake_case と不整合だが署名済み content と
   SQLite に永続済み。「統一リファクタ」は全既存データの破壊。固定: 同上 wire_snapshot.rs。
5. **docs エントリ key スキーム**(`objects/{id}/state` 等): app-api の `stable_key` 生成と
   cn-indexer の prefix 走査の暗黙契約。固定: cn-indexer の ingestion_contracts(常時実行)。
6. **永続スキーマと identity**: SQLite / Postgres の migration 済みスキーマ、identity ファイル群
   (keyring account は db パス依存 — パス変更 = 鍵ロスト)、endpoint secret(iroh::SecretKey の
   serde 表現に直結)。固定: migration テスト(拡充は後続 WP)。
7. **community-node HTTP 契約と manifest**: cn-user-api の endpoint 群と `CommunityNodeManifest`
   (server 完全版 ⇔ client slim 版)。変更は後方互換な optional フィールド追加のみ可。
   固定: `crates/cn-user-api/tests/`、round-trip は
   `crates/desktop-runtime/src/community_node/manifest_support.rs` +
   `crates/cn-operator/tests/manifest_golden.rs`(**この 2 ファイルを触る PR は
   `cargo xtask rust-test` の round-trip テストと `cn-test` の golden の両方を実行する**)。
8. **cn-operator の capability 提供状態と公開条件**: `community_index` / `moderation` /
   `community_local_trust` は #616 の readiness / fail-closed gate を経て #617 で
   `Availability::Available` へ移行済み（[ADR 0025](docs/adr/0025-community-node-indexing-foundation.md)
   2026-08-16改訂、`crates/cn-operator/src/capability.rs::availability`）。提供可能であることと
   個々の配備で有効・公開であることは区別し、設定と有効な readiness 記録の条件を維持する。
   ADR 0027 §2.9 の昇格条件は当時の判断として保持する。将来向けの `Planned` 区分を削除せず、
   リファクタで availability、manifest の表明、operator の有効化条件を変えない。
9. **Tauri IPC 契約**(`crates/app-api/src/views.rs` ⇔ `apps/desktop/src/lib/api/types.ts`):
   同一バイナリ内契約のため両側同時変更なら改名可能だが、片側変更は silent break。
   固定: 高頻度 8 型グループは共有 fixture(apps/desktop/src/lib/api/__fixtures__/views/)を
   Rust 側 crates/app-api/src/tests/views_wire_snapshot.rs と TS 側 viewsContract.ts(+ .test.ts)が
   両側から検証する(再生成手順は各ファイル冒頭)。入力方向(requests.rs ほかの request DTO)は
   WP-B6 で codegen 対象になり、runtimeApi.ts の request literal が生成型への `satisfies` で
   拘束される(再生成 diff + tsc の二重検出)。CommunityNode 系 view の fixture 化は未。

## 地雷リスト(直したくなるが、してはいけない)

互換性境界とは別に、「一見 dead code / 不整合 / 冗長に見えるが、消す・揃える・golden 化すると
壊れる」ものを挙げる。判断に必要な理由と検出経路は各項目へ集約し、repository 外や非追跡の
計画を前提にしない。

1. **`ConnectionPath::RelayFallback` は削除禁止の第3経路**: `AGENTS.md`「通信経路」節が規定する
   `Direct P2P -> Relay Supported P2P -> Relay Fallback` の 3 番目。variant は
   `crates/transport/src/config.rs` に定義され、TS 側表示分岐(生成型
   `apps/desktop/src/lib/api/types.generated.ts` / `apps/desktop/src/shell/presentation.ts` の
   `relay_fallback`)も実在する。判定は実装済み(issue #573):
   `crates/transport/src/iroh/peer_state.rs` が snapshot 時に `Endpoint::remote_info` の
   active transport addr を観測し、topic の全 connected peer が relay 経由でしか
   実データを流せない場合に RelayFallback と `fallback_peer_ids` を報告する
   (実 relay 検証は `crates/transport/src/iroh/tests/relay_connectivity.rs` の
   relay-only テスト)。**variant は削除・改名しない**。serde 表現(`relay_fallback`)は
   IPC 契約であり、既存 2 経路(direct / relay_supported)の判定を変える変更は
   characterization テスト(`peer_state.rs` の unit test)を先に更新しない限り禁止。
2. **`apps/desktop/src/styles/shell-scoped-overrides.css`は現役の本番 CSS**:
   portalとshellで意図した実効値差がある。重複という見た目だけで削除しない。
   配置・cascadeと統合時の確認は[UI実装配置](docs/architecture/desktop-ui-implementation.md)の
   「Portalとscoped override」、実行する検証は本書のマトリクスを参照する。
3. **署名バイトは非決定的なので golden 化しない**: `KukuriKeys::sign_schnorr`
   (`crates/core/src/crypto.rs`)は毎回 aux_rand を生成するため署名バイトは実行毎に変わる。凍結対象は
   canonical バイト列と「過去 fixture の verify 継続」であり、署名バイトを snapshot golden にすると
   毎回 fail する(理由は `crates/core/src/tests/signing_canonical.rs` の doc）。

## 互換パスと sunset 条件

以下は後方互換のために残している経路で、「いつ削除してよいか」を明文化せずに消すと既存データ・
既存ユーザーが壊れる。撤去条件は WP-C8(2026-07-07)で確定した。条件を満たすまで削除しない
(このリストが「消してよいか」の判断入口。コード側の該当箇所にも同じ条件をコメントで明記している)。

| 互換パス | 所在 | 誤削除の影響 | sunset 条件(WP-C8 で確定) |
|---|---|---|---|
| 旧 `.nsec` 鍵ファイル読込 | `crates/desktop-runtime/src/identity.rs`(`legacy_key_file_path`。テスト `legacy_nsec_file_still_loads` が固定) | **利用者が気づかないまま別人の鍵になる**: 旧ファイルは読み込んでも新形式へ再保存されず残り続け、鍵が見つからないと `load_or_create_keys` が黙って新しい鍵を生成するため | **撤去する際は、`.nsec` ファイルを検知したら起動を止めて案内を出す処理(fail-loud)とセットで行うこと**。黙って新しい鍵を生成する現状のまま読込パスだけを消してはならない。鍵は preview 段階でも黙って失ってよいものではない |
| epoch `"legacy"` 互換 | `crates/app-api/src/service/projection_support.rs`(`legacy_epoch_id` / `private_channel_replica_for_epoch` / `joined_private_channel_state_from_capability`) | epoch 導入前に保存されたプライベートチャンネル capability(epoch_id が空)のチャンネル履歴が読めなくなる。現行ビルドが空の epoch_id を新規に書くことはない | **正式リリースの節目で削除可**(preview データの保全は保証しない方針)。削除時は空の epoch_id を持つ capability をエラーで拒否すること(黙って読めなくならないようにする) |
| ~~CRLF checksum 自己修復~~ | ~~`crates/store/src/sqlite/connection.rs`~~ | - | **撤去済み**(2026-07-06、WP-C6 / PR #485)。checksum 不一致は fail-loud で起動失敗し、回帰テストで固定済み |

**互換パスではないと再分類したもの(WP-C8):**

- `resolved_urls` の未解決時の扱い(`crates/desktop-runtime/src/community_node/requests_support.rs`。
  認証時に base_url で代用する fallback と、解決されるまで再取得を繰り返す判定)は、旧設定の互換では
  なく**恒常経路**。コミュニティノードを新規追加した直後は必ず未解決(None)であり、この経路が初回接続
  そのものを支えている。削除対象ではないため sunset 条件は持たない。

## 真実の置き場所ルール

正本、読む順番と優先関係、承認済み変更要求、過去記録の扱いは[docs/README.md](docs/README.md)の「正本と変更要求」に従う。

## path別検証マトリクス

受入条件と変更影響から必要な検証を選ぶ。ローカルでは関連test・構文・型検査を行い、全体確認はPR CIへ委ねる。配置pathだけで全suiteを必須にしない。CIにない必要な実機・OS固有検証は実行先を明示する。
実行方法は[開発手順](docs/runbooks/dev.md)、UIの確認は[ADR 0014](docs/adr/0014-uiux-dev-flow.md)を参照する。これらも合意した条件に適用し、未依頼のエッジケースを追加しない。

| 変更path / 内容 | 選ぶ検証 |
| --- | --- |
| `crates/core/**`、`store/**` | 変更module・公開契約の関連test。永続化変更は該当migration往復・索引・データ保持 |
| `crates/transport/**`、`docs-sync/**`、`iroh-node/**`、`blob-service/**` | 変更経路の関連test。実peerの振る舞いが条件にあれば該当connectivity test / scenario |
| `crates/app-api/**`、`desktop-runtime/**` | 対象の操作から結果までの関連test。公開DTO・IPC変更は消費側を含める |
| `crates/cn-*/**` | 対象crateの関連testと必要なDB / provider fixture。全CN suiteはCI |
| `harness/scenarios/**` | `cargo xtask scenario <changed-scenario>` |
| `apps/desktop/**`（`src-tauri/**`以外） | 関連frontend testと型検査。描画変更は対象surfaceのbrowser / visual確認 |
| `apps/desktop/src-tauri/**` | 関連`cargo xtask tauri-test -- <filter>`とcompile。`cfg(windows)`の必要なtestはWindowsで確認 |
| 文書・コメント | `git diff --check`、変更記述・リンク・例示commandの照合。runbookに載るdeploy・削除操作は検証目的で実行しない |
| 設定・雛形・機械検査用ミラー | 構文、読み込み先、対応する既存の関連検査 |

実Iroh結合testが必要な場合、`cargo xtask app-api-slow-test`の全体はnightlyの対象であり、PR CI成功だけではその実行を証明しない。関連testをローカルまたは必要な別環境で選んで実行する。

`cargo xtask e2e-smoke` は `desktop_smoke_post_persist`（`FakeNetwork`・in-process 単一 runtime）1 本を回す desktop 永続 smoke であり、post 作成 → timeline → restart → 再表示の永続往復のみを検証する。実 transport / docs-sync の peer 間経路・relay replication は通らない。peer / replication / CN 接続の検証は該当crateの関連test（実irohを使うものを含む）と `cargo xtask scenario <connectivity scenario>` が担う。CI 実配線の connectivity scenario は `community_node_public_connectivity`（fast / release）、加えて `community_node_multi_device_connectivity`（nightly）。connectivity scenario は cn-postgres 起動 + 実 iroh peer 複数で重いため、マトリクスでは「振る舞いが変わる場合」の条件付き要求とする。

### 検証の選定・中断

- 実行前に、変更に必要な検証、利用可能なOS・依存、既存結果で確認済みの範囲を特定する。ローカルで重すぎる・実行不能な場合は最も狭い関連commandを先に使い、未実行の必須検証と理由、CIまたは別環境で補う方法を記録する。狭い検証の成功を全suiteの成功とはしない。
- 実行前に確認対象と終了条件を定める。ユーザーの停止指示、資源枯渇、ハング、前提誤り、受入条件に不要な実行と分かった場合は中断する。一時的な無出力だけでハングと断定せず、完了済み範囲・未確認範囲・再開条件を記録する。
- 必須CI、適用される独立監査・境界検証を満たす前に完了・マージ可能と報告しない。実行不能や中断は未確認のまま残す。必要な検証が成功した後は、対象変更・新たな失敗・未解決の懸念がなければ同じ検証を繰り返さない。

必要な検証が成功したら終了する。PR作成前の全suite反復を追加の完了条件にしない。

## 大型ファイルポリシー

`cargo xtask oversized-files` は大型(1000行以上)の手書きファイルを報告し、
`xtask/oversized-baseline.json` との比較で **ratchet 方式の CI ゲート**として動作する。

ゲートの判定:

- baseline に無い 1000 行以上の新規ファイル → **fail**。
- baseline 記載ファイルが記録行数を超えて成長 → **fail**。
- 行数不変・減少 → pass(減少時は baseline 更新推奨の note を表示)。
- baseline の更新は `cargo xtask oversized-files --update-baseline` で再生成し、
  正当化を PR 本文に書いた上でコミットする(レビューを通すことが ratchet の意図)。
- ファイルを分割して 1000 行未満にした場合も `--update-baseline` で baseline から除去する。

ルール:

- 明示的な正当化(= baseline 更新コミット)なしに、1000行以上の新規手書きファイルを追加しない。
- 大型ファイルでも最終コード量の最小化を優先し、重複の統合・置換・削除を検討する。行数が増える場合は既存ratchetのbaseline更新と、受入条件に必要な理由を示す。
- 大型ファイルで複数責務に触る場合は、変更の理解・検証・差し戻しを妨げる具体的な構造問題を確認する。分割が必要なら別の構造整理として計画し、行数だけで分割を必須にしたり、依頼外の分割を混ぜたりしない。ratchetのCIゲートは維持する。
- formatting-only change と semantic change を混ぜない。
- generated file、lock file、icon は明示的に対象化されない限り、この方針の対象外とする。

## AI利用時の証拠規範

- 「観測した事実」「そこからの推測」「未検証事項」を区別する。説明の詳しさや確信度を証拠にしない。
- 「未使用」「重複」「全 test 成功」「互換性に影響しない」は仮説として扱い、参照、型検査、実行ログ、
  test / contract / scenario で確認する。
- 静的検索だけで dead code や意味が同じ重複と断定しない。候補には path、symbol、利用者、入口、
  保護する test を可能な限り添える。
- 新しいhelper、wrapper、抽象化を作る前に既存の同一責務を確認し、統合・置換後の総量を比較する。現行様式の温存を最終コード量より優先しない。
- 一般的な clean code 論や将来拡張だけを根拠に、新しい層、型、ファイルを増やさない。
- 提案した API、command、dependency が実在することを確認する。リファクタリング PR で dependency を追加しない。
- test は観測可能な挙動を固定する。実装順序や source 構造の変更検知だけを目的とする test は追加しない。
  既存 wire / serialization / visual contract を固定する snapshot はこの限りではない。
- 変更と同時に追加した test だけを挙動維持の証拠にせず、変更前の挙動、既存 contract、fixture、scenario と照合する。
- 必要なtestの削除・skip、assertionの弱体化、warning抑制、hardcodeによって成功扱いにしない。重複・廃止testの整理は検証範囲を対応付けて行う。
- 高リスクな境界変更は人間または独立した別コンテキストでも review する。複数担当の合意だけを挙動維持の証拠にしない。

## 記録テンプレート

定期監査では次を記録する。

```md
## 対象と開始trigger

## 有限scopeと終了条件

## 変更前baseline

## 候補一覧

| 候補 | 観測した問題と根拠 | 目標成果 | 優先順位 | 分類 | 理由 / 個別Issue |
|---|---|---|---|---|---|

## 監査終了判定
```

個別作業では次を記録する。互換性境界、互換パス、validation の詳細は本書の既存節を参照し、
テンプレート内へ複製しない。

```md
## 種別

## 観測した問題と根拠

## 現在の責務・依存・挙動

## 目標とする構造上の成果

## 維持する挙動

## 対象 / 対象外

## 互換性境界・互換パス

## 変更前baselineと成功判定

## Issue / 段階 / PR分割

## Validation

## 差し戻し手順

## 見送った候補・別Issue
```

## 必須AIワークフロー

リファクタリング監査または個別作業では、AIエージェントは以下の順序に従う。

1. `AGENTS.md`、この文書、対象の ADR / runbook / test / scenario を読む。
2. 対象領域の構造を把握し、候補と観測根拠を収集する。
3. 候補を分類・優先順位付けし、一つの意図を選ぶ。監査だけなら固定 scope の分類完了で終了する。
4. 現在の挙動、構造上の問題、目標成果、対象外、互換性境界、互換パス、validation を記録する。
5. 影響 path と必須 validation を特定し、変更前に実行して既知の失敗を今回の回帰と区別できるよう記録する。
6. 固定した受入条件の検証が不足する場合は、必要なtestを変更前に確認する。testと実装・共通化・削除は同じ成果のPRにまとめられる。未依頼のケースを増やさない。
7. 一つの成果を検証・差し戻しできる段階で実装する。最終形への接続と旧処理の撤去まで計画し、差分を小さくするだけの分割をしない。各段階で関連validationを実行する。
8. 停止条件を検出したら先へ進まず、再調査または別種別へ再分類する。
9. 最後に path 別検証マトリクスの validation と、差分全体の review を行う。未実行項目は理由を報告する。
10. before / after の構造上の成果、挙動維持の証拠、残存リスク、見送った候補を完了報告へ記録する。

## レビューチェックリスト

本書の「個別作業の成果物ベースの完了条件」に証跡を対応付ける。特に互換性境界の見落としを防ぐため、
diff内の `#[serde` 属性、wire文字列、canonical実装を確認する。共通の監査判定・停止条件はIssue運用手順に従う。

## 完了報告形式

本書の「記録テンプレート」を更新し、結果・構造上の成果・挙動維持の証拠・検証結果・残存リスクを要約する。
詳細へのリンクで足り、別形式へ全項目を転記する必要はない。非該当欄は省略できるが、必須検証の未実行・失敗・中断は理由と補完方法を残す。
