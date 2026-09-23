# Community Node Shutdown と User Continuity

最終更新日: 2026-09-24

## この文書の位置づけ

この runbook は、community node operator が node を**安全に停止する手順**と、user が node 停止後も **identity / profile / social graph を失わない**ことの両方を、同じ文書で扱う。

kukuri の community node は Mastodon 的な home server ではなく、P2P-first network の補助層である（前提: `docs/architecture/p2p-first-community-node-responsibility-boundary.md`）。したがって shutdown も「アカウントの引っ越し」ではなく「補助 capability の停止」として設計する。

関連:
- community node manifest / authority scope / capability scope
- public manifest endpoint
- client settings の node 依存度表示
- operator docs generator（`docs/runbooks/community-node-operator-docs.md`）

この文書は法的助言ではない。

## 0. 大原則: node 停止は user の存在を終わらせない

- user を識別する canonical source は **user 自身の鍵**であり、いずれかの community node ではない。
- nodeの停止はuser identityの失効や、端末に保存済みのprofile / social graphの削除を意味しない。未取得データの再取得は、提供するpeerや保持状態に依存する。ネットワーク上に必ずcopyがあることや全履歴の復元は保証しない。
- したがって community node を「アカウントの所在地」「退会先」「凍結権者」として停止設計してはならない。停止するのは node が提供していた**補助 capability** だけである。

## 1. User side: 停止後も残るもの / 失われ得るもの

### 停止後も残るもの（node-owned ではない）

| 項目 | 理由 |
|---|---|
| user identity（鍵） | user の鍵に紐づく。node に属さない。 |
| 取得済みsigned profile | author docs + signed envelopeが正本。端末のcopyはnode停止で消えないが、欠損分の再取得は提供元に依存する。 |
| 取得済みsocial graph / follow edges | 端末にある署名済みstateを利用する。未取得の全edgeを揃える保証はない。 |
| local cache / local projection | client ローカルに保持される。 |

必要な対象の欠損分だけを、利用者が選んだ別nodeや到達可能なpeerから取得する。全author・全topicの走査や再同期の完了を継続利用の前提にしない。本人性の復元には本人の鍵、端末データの移行にはバックアップを使う。

### 停止で失われ得るもの（capability 別）

node-local な補助 capability は、その node の停止で利用できなくなる。端末の保持済みデータとは区別し、nodeだけが持っていた情報や到達不能なcopyの再取得は保証しない。

| capability | 停止時の影響 | user から見た代替 |
|---|---|---|
| bootstrap assist | この node 経由の初期 peer 探索ができなくなる | 別の bootstrap node / 既知 peer / DHT |
| relay assist（iroh relay / traffic fallback） | この node 経由の NAT 越え補助が止まる | 直接 P2P / 別 relay node |
| topic rendezvous | この node 経由の topic peer 合流が止まる | 別 rendezvous node / 既存 peer |
| node-local index（community index） | この node の検索・発見結果が消える | 別 index node / ローカル projection |
| node-local moderation labels | この node の moderation label が参照できなくなる | 別 node の label（optional trust input） |
| node-local trust signals | この node の trust signal が参照できなくなる | 別 node の signal（optional trust input） |
| node-local media cache | この node の cache 経由の媒体配信が止まる | 原本 blob / 別 cache node |

重要: これらはいずれも **node-local な補助出力**であり、停止によってuserの鍵や端末の保持済みstateを削除するものではない。moderation labels / trust signals は元々 optional trust input（`docs/adr/0027-deterministic-moderation-critical-safety.md` §2.1）であり、network-wide な認定ではない。そもそも network-wide な認定は P2P 基盤上に中央権者が存在しないため構造的に成立し得ない。

## 2. Operator side: 安全な停止手順

停止対象のnode・capability、停止日時、保持するデータと削除対象、確認方法を先に固定する。対象サービスの停止、残す資料・連絡先と保持期限の記録で本作業を終え、全peerへの通知到達や全データ移転を完了条件にしない。

### 推奨手順（順序）

1. **停止予定と影響するcapabilityを案内する**。利用可能な公開文書・連絡経路を使う。後述のmanifest案や追加client UIの実装をnode停止の前提にしない。
2. **public listing / default listing からの撤退**。default-onboarding-node に登録されている場合は、その登録元から外す手続きを行う（network 全体を統治する中央権者は構造的に存在せず、この node も network-wide authority ではないため、外しても network は継続する）。
3. **capability を段階的に停止**する。利用者影響の小さいものから順に止めるとよい。各 capability の停止方針:
   - auth / consent: 新規 token 発行を止める。既存 token は失効まで有効。
   - bootstrap assist: heartbeat 受付を止め、bootstrap node 一覧から外す。
   - relay assist / traffic relay fallback: relay 受付を止める。直接P2Pや別relayへの再接続が可能か確認する。代替先がなければ未接続のままとなる。
   - topic rendezvous: rendezvous 受付を止める。
   - community index / moderation / trust signal: 新規 index / event 発行を止める。既存 signed moderation event は配布物として残せる。
   - report endpoint: 通報受付を止める。abuse contact は最終窓口として案内を残すか、shutdown notice に移行先を記載する。
4. **retention policy に沿ってデータを扱う**。connection logs / moderation logs / report data は、生成された operator docs（data-retention-policy.md）の保持期間に従って削除または保全する。signed moderation event は監査用に保持方針を明示する。
5. **generated operator docs の final export** を用意する（`cargo run -p kukuri-cn-operator --bin cn-operator -- generate-docs`）。停止時点の manifest / policy / retention を最終版として保存し、説明責任を果たせるようにする。
6. **DB / インフラの停止**。`docs/runbooks/community-node-self-host-vps.md` の起動手順の逆順で停止・バックアップする。

### retention の扱い

- connection logs / moderation logs: data-retention-policy.md の保持期間で削除する。
- report data: 保持期間に従う。未対応の通報がある場合は移行先 / abuse contact を shutdown notice に記載する。
- signed moderation event: issuer node の判断記録として、監査可能性のために保持方針を明示する（即時削除 / 一定期間保持のいずれか）。

## 3. Manifest shutdown notice の形

以下はmanifest拡張の提案であり、現在の配布物に対応があるとは扱わない。採用は明示的な機能変更として対象と受入条件を別途定める。node停止作業から自動的に実装対象へ追加しない。提案する形:

```json
{
  "shutdown_notice": {
    "status": "active | winding_down | retired",
    "effective_at": "2026-07-01T00:00:00Z",
    "message": "This node will stop bootstrap/relay assist on 2026-07-01.",
    "affected_capabilities": ["bootstrap_assist", "iroh_relay", "topic_rendezvous"],
    "alternative_contact": "ops@example-kukuri.net",
    "alternative_nodes": ["https://another-node.example"]
  }
}
```

- `status`: `active`（通常）/ `winding_down`（停止予告中）/ `retired`（停止済み）。
- `affected_capabilities`: 停止対象の capability キー（manifest の capability scope と同じ語彙）。
- `alternative_nodes`: 任意。client が代替 node 追加導線に使える候補。強制ではなく参考。

採用時はpublic manifestから取得する公開情報として扱い、private secretを含めない。

## 4. Client UX

次は前節の拡張を採用する場合のUX案であり、現行機能の完了報告や停止作業の追加条件にはしない。

- settings の node dependency 表示に **affected capabilities** と shutdown status / effective_at を表示する。
- manifest の **shutdown notice message** を表示する。
- **alternative node を追加する導線**を提示する（`alternative_nodes` があれば候補表示。なければ手動追加への導線）。
- **identity / social graph は node-owned ではない**ことを説明する（停止しても user は失われない、という安心を明示する。client settings の境界説明と同じトーン）。

client は、shutdown を「アカウント凍結 / 退会」ではなく「補助 capability の停止と代替手段の案内」として表示する。

## 5. generated operator docs との連携

- 停止時の説明責任は operator docs generator の生成文書で果たす。final export（manifest / terms / privacy / retention / abuse-policy / moderation-policy）を停止時点版として保存する。
- shutdown notice の retention 方針は data-retention-policy.md と整合させる。
- capability 別の停止影響は、operator docs runbook（`docs/runbooks/community-node-operator-docs.md`）の capability 一覧と同じ語彙で記述する。

## 受け入れ条件との対応

- community node shutdown と user continuity を同じ文書で扱っている → 本文書全体
- operator side の停止手順がある → 2
- user side に残るもの / 失われるものが明確になっている → 1
- identity / profile / social graph が node-owned ではないことを説明している → 0, 1
- node-local capability 停止時の影響を capability 別に説明している → 1（表）, 2
- manifest shutdown notice の形が定義されている → 3
- client settings / node dependency display と連携できる → 4
- generated operator docs と連携できる → 2, 5

## 関連

- `docs/architecture/p2p-first-community-node-responsibility-boundary.md`
- `docs/runbooks/community-node-operator-docs.md`（operator docs generator）
- `docs/runbooks/community-node-self-host-vps.md`（起動・停止インフラ手順）
- community node manifest authority scope / capability scope
- client settings の node 依存度表示
- `docs/adr/0027-deterministic-moderation-critical-safety.md`（moderation event の trust semantics / advisory ≠ command）
