<p align="center">
  <img src="resources/icons/Ugurugu.png" width="112" alt="Ugurugu アプリアイコン">
</p>

<h1 align="center">Ugurugu</h1>

<p align="center">
  絵がゆらゆら動き出すお絵かきアプリです。
</p>

<p align="center">
  <a href="https://github.com/nyabi-gh/Ugurugu/releases/latest"><img src="https://img.shields.io/github/v/release/nyabi-gh/Ugurugu?style=flat-square&color=ffc94a" alt="Latest release"></a>
  <a href="https://github.com/nyabi-gh/Ugurugu/releases"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnyabi-gh%2FUgurugu%2Fdownload-badge%2Fdownloads.json&style=flat-square" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-ffc94a?style=flat-square" alt="License"></a>
</p>

<p align="center"><a href="README.md">KR</a> · <a href="README.en.md">EN</a> · <b>JP</b></p>

完成した作品は繰り返し再生されるGIF・WebPや背景が透明な画像として保存でき、
WiggleWiggleToolで作った`.wawa`の絵も引き続き編集できます。

## ダウンロード

| プラットフォーム | 対応環境 | ダウンロード |
| --- | --- | --- |
| Windows | Windows 10以降、64ビット | [Setup.exe](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-Windows-x64-Setup.exe) |
| macOS | macOS 14以降、Apple Silicon | [DMG](https://github.com/nyabi-gh/Ugurugu/releases/latest/download/Ugurugu-macOS-arm64.dmg) |

- **Windows**: Setupファイルを実行してください。確認の警告が出た場合は
  **詳細情報 → 実行**を選びます。
- **macOS**: DMGを開き、UguruguをApplicationsフォルダーへドラッグして
  ください。Appleの確認を受けてから配布されます。

必ず公式の[Releasesページ](https://github.com/nyabi-gh/Ugurugu/releases/latest)から
ダウンロードしたファイルを使用してください。リリースページのその他のファイルは
自動アップデート用なので、ダウンロードする必要はありません。

<details>
<summary>システム要件</summary>

| 項目 | 最小 | 推奨 |
| --- | --- | --- |
| OS | Windows 10 64ビット / macOS 14（Apple Silicon） | Windows 11 / 最新のmacOS |
| メモリ | 8GB | 16GB以上 |
| グラフィック | 特別な要件なし | Direct3D 11（Windows）またはMetal（macOS）対応GPU |
| 入力デバイス | マウス | 筆圧対応のペンタブレット（Wacomなど） |

キャンバスの表示・拡大・移動・再生はGPUで処理し、グラフィック
アクセラレーションが使えない場合は自動的にソフトウェアレンダリングに
切り替わります。大きなキャンバス（最大4096×4096）で複数のレイヤーを使うときは、
メモリに余裕があるほどプレビューがなめらかになります。

</details>

## はじめて使うとき

1. 新しいキャンバスを作るか、以前の絵を開きます。
2. 左側からブラシを選び、線を描きます。
3. **ゆらぎ**パネルで動きを選び、`P`を押して再生します。
4. **ファイル**メニューからGIF・WebPまたはPNG・JPGとして書き出します。

困ったときは`F1`でアプリ内ヘルプを開いてください。

## 主な機能

- **ゆらぎ** — クラシック・スムーズ・ステップの3つの動き、揺れ・ディテール・
  連動・ランダム性の調整、途切れ線の効果。レイヤーごとにオン・オフできるので、
  背景を止めたまま線だけを動かせます。
- **描画** — 筆圧対応のブラシ・消しゴムを含む17種類の道具、手ぶれ補正、
  塗りつぶし、自由形・四角形・楕円の選択と変形、マルチタッチディスプレイでの
  2本指による移動・拡大・回転。
- **レイヤー** — グループ、不透明度、合成モード、画像の読み込み、キャンバスの
  切り抜き・拡張・サイズ変更。
- **保存と書き出し** — 作品は`.ugu`で保存（以前の`.wagle`・`.wobble`も開けます）。
  透明背景のGIF・WebP・PNG・JPGに書き出し、`.wwpreset`で設定を共有、
  予期せぬ終了後の作業復元。
- **カスタマイズ** — パネルのドラッグで縦に重ねる・横に並べる・タブにまとめる、
  アクセントカラー、すべてのショートカットの変更、日本語・韓国語・英語の画面、
  アプリ内アップデート。

設定はツールバーの歯車ボタン（またはWindowsの**編集 → 設定**、macOSの
**Ugurugu → 設定**）から開けます。**ウィンドウ → パネル配置をリセット**で
最初の配置に戻せます。新しいバージョンは起動時に自動で確認され、**ヘルプ →
アップデートを確認**から手動でも確認できます。

<details>
<summary>ゆらぎの設定をくわしく</summary>

パネル上部の範囲を**アクティブレイヤー**に切り替えると、選んだレイヤーだけに
適用されます。

| 設定 | 範囲（初期値） | 説明 |
| --- | --- | --- |
| モーションスタイル | クラシック · スムーズ · ステップ（クラシック） | **クラシック**は以前のバージョンと同じ動き、**スムーズ**はポーズの間を流れるようにつなぐ動き、**ステップ**はポーズをぱっと切り替える手描きアニメ風の動きです。 |
| 揺れ | 0 ～ 12 px（1.6 px） | 線が描いた位置から離れる最大距離。0 では動きません。 |
| ポーズ数 | 1 ～ フレーム数（8） | 繰り返し使う別々の絵の枚数。少ないほどカクカク、多いほどなめらかです。1 では止まります。 |
| ディテール | 1 ～ 24（12） | 線に沿った揺れの間隔。低いとゆるやかな波、高いと細かく震える感じになります。 |
| 連動 | 0 ～ 100%（100%） | 線どうしが一体になって動く度合い。100% では絵全体が一緒に、0% では線ごとにばらばらに動きます。 |
| ランダム性 | 0 ～ 100%（0%） | 上げるほど点ごとに跳ねるざらついたノイズになります。 |
| 途切れ線 | オン · オフ（オフ） | 線の一部が消えたり現れたりします。下の 2 つはこれをオンにしたときだけ効きます。 |
| 途切れ量 | 0 ～ 100%（35%） | 線が消える量。100% では線がまるごと消えます。 |
| 途切れ範囲 | 2 ～ 256 px（24 px） | 途切れるかたまりの大きさ。小さいと点線のように、大きいとごっそり途切れます。 |

**ポーズ数**と**ディテール**は*スムーズ*・*ステップ*でのみ効き、*クラシック*では
使われません。

</details>

## WiggleWiggleToolの絵を続ける

WiggleWiggleTool 10で保存した`.wawa`ファイルを**ファイル → 開く**から読み込めます。
元のファイルは変更せず新しい作品として開き、最初の保存時に同じ名前の`.ugu`が
提案されます。二つのアプリでは描き方が異なるため、一部の揺れ・エアブラシ・
塗りつぶした形は少し違って見えることがあり、変更された項目や読み込めなかった
項目はアプリが知らせます。

## ショートカット

よく使う初期設定です。**設定 → ショートカット**ですべて変更できます。

| キー | 動作 |
| --- | --- |
| `B` / `E` | ブラシ / 消しゴム |
| `L` / `W` / `G` | 範囲選択 / 自動選択 / 塗りつぶし |
| `I` または `Alt` + クリック | 色を取り出す |
| `P` | 再生 / 一時停止 |
| `Space` + ドラッグ、スクロール | キャンバスを移動、拡大・縮小 |
| `Ctrl/Cmd+Z` | 元に戻す |
| `F1` | ヘルプ |

<details>
<summary>すべてのショートカット</summary>

| キー | 動作 |
| --- | --- |
| 2本指のドラッグ・ピンチ・ひねり | キャンバスを移動・拡大縮小・回転（対応するマルチタッチディスプレイ） |
| `Shift+Space` + ドラッグ | キャンバスを自由に回転 |
| `Shift` + スクロール | キャンバスを5°ずつ回転 |
| `-` / `^` | キャンバスを左/右に5°回転 |
| **表示 → キャンバスを回転 → キャンバスの回転をリセット** | 回転を0°にリセット |
| `Alt+Delete` | 選択範囲をブラシの色で塗りつぶす |
| `Ctrl+T` | 下のアニメーションバーを折りたたむ / 表示する |
| `Ctrl/Cmd+C`, `X`, `V` | コピー / 切り取り / 貼り付け |
| `Ctrl+Y`（Windows）、`Cmd+Shift+Z`（macOS） | やり直す |
| `Ctrl/Cmd+0` | キャンバスをウィンドウに合わせる |
| `Ctrl/Cmd+1` | 実際のピクセルサイズで表示 |
| `Enter` / `Esc` | 変更を適用 / キャンセル |

</details>

## 問題の報告

[GitHub Issue](https://github.com/nyabi-gh/Ugurugu/issues)に、何をしていたか、
実際に何が起きたかを書いてください。`.ugu`ファイルを添付していただけると大変
助かります。セキュリティの脆弱性は公開Issueではなく、
[セキュリティポリシー](SECURITY.md)に記載の非公開の窓口へお知らせください。

## 開発とコントリビューション

ソースからのビルドは[BUILDING.md](BUILDING.md)、コントリビューションの方法は
[CONTRIBUTING.md](CONTRIBUTING.md)を参照してください。

## クレジットとライセンス

- Development support by seuppi
- App icon artwork by seuppi（`resources/icons/`、GPL-3.0-or-laterで配布）

Copyright (C) 2026 Nyabi (nyabi-gh)

このプログラムはフリーソフトウェアで、[GNU General Public License](LICENSE)
バージョン3、または（選択により）それ以降のバージョンの条件に従って再配布および
改変できます。いかなる保証もありません。コントリビューションも同じ条件で受け付け、
同梱のフォントとライブラリの条件は[サードパーティ通知](THIRD_PARTY_NOTICES.md)に
あります。

SPDX: `GPL-3.0-or-later`
