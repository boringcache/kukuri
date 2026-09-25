# desktop の blob と remote 内容の保持

現行の受入条件と予算は [#1221](https://github.com/kukuri-app/kukuri/issues/1221) の R3-B/C・R5-A に従う。本書は保存先と取得経路の対応を示す。旧 #1207 時点の試行回数や無期限 cache の記録は現行仕様ではない。

| 所有者 | 内容と取得 | 保持・回収 |
| --- | --- | --- |
| Iroh SDK `blobs.db` | 本人が書いた本文・添付、pin された資産、切替前の旧内容 | 保護対象の移行・backup は R5-G、閉じた旧領域の退役は R5-I。旧領域を R5-A の容量達成のために一括削除しない |
| account SQLite の remote cache | 検証済み公開bucket record、remote blob、投稿projection・索引と表示用ラベル根拠 | 非保護分を合計1GiB、非利用7日、1処理128件以内で回収。取得中は1MiB単位の予約を計数し、容量不足では取得を延期する。bookmarkが参照する共有blobは参照が残る間保護する |
| 画面の object URL | 表示需要に対して取得したbytes | 表示対象から外れたhashと、成人向けラベルの最後の根拠が回収されたhashを破棄する。表示URL/bytesの総量上限はR1-Cが担当する |

desktop の通常remote blob取得は一時bytesを返し、それだけでは保存しない。本文・添付・sessionのconsumerがaccount/参加世代・対象hash・取得gateを確認してから `put_remote_blob` でcacheへ書く。local状態の確認はremote I/Oを起こさない。SDKに同じhashの保護blobがあれば二重保存しない。cacheにあるblobはhash指定の実QUICで別peerへ再提供できる。公開bucketのrecordは署名とscopeの検証後だけcacheへ確定し、SDK namespaceのimportや定常syncを始めず対象キーで再読込・再提供する。

成人向け表示設定がOFFの対象は、Rust側でbytes取得前に止める。ONで取得した成人向け添付は一時bytesのまま扱う。remote投稿projectionや表示用ラベルの根拠が回収され、現在の対象から再検証できない添付は `None` として非表示にする。ラベル回収通知が届いた画面は表示済みURLも破棄する。既存の保護投稿に結び付くラベルと旧保存領域の移行は通常remote cacheとは別に扱う。

`blob_objects` の永続状態表は読取り先が無かったため撤去した。添付の表示状態は現在のBlobServiceのlocal状態から求める。欠損していても投稿と操作を続け、表示中の本文・返信先・sessionの再試行は #1221 R3-B の最大4試行・5/30/120秒・需要消失時停止に従う。画面のURL/bytesの最終予算とlarge mediaの分割処理はR1-C、旧SDK保存領域の保護移行と物理退役はR5-G/Iで確認する。
