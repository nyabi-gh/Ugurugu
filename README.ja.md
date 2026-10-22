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

線を引くだけで、手描きアニメーションのように絵が生き生きと動き出します。
動く絵は GIF や WebP で保存して、どこにでも投稿できます。

> [!NOTE]
> Ugurugu 3.0 は Windows 専用です。macOS では 2.2.13 をお使いください。
> 2.1.0 以降に保存した `.ugu` ファイルは 3.0 で開けます。それより古いファイルは 2.2.13 で一度保存し直してください。

## ダウンロード

| プラットフォーム | 動作環境 | ダウンロード |
| --- | --- | --- |
| Windows | Windows 10 以降、64 ビット | [最新リリース](https://github.com/nyabi-gh/Ugurugu/releases/latest) |
| macOS (2.2.13) | macOS 14 以降、Apple Silicon | [DMG](https://github.com/nyabi-gh/Ugurugu/releases/download/v2.2.13/Ugurugu-macOS-arm64.dmg) |

インストール時に「Windows によって PC が保護されました」と表示されたら、**詳細情報 → 実行** を選んでください。

## はじめに

1. ブラシを選んで線を描きます。
2. **ゆらぎ** パネルで動きを選び、`P` で再生します。
3. **ファイル** メニューから GIF や WebP で書き出します。

わからないことがあれば `F1` でヘルプを開いてください。

## できること

- **ゆらぎ** — 動きの強さや雰囲気を選び、レイヤーごとに動かしたり止めたりできます。
- **アニメーション** — レイヤーにキーを置いて移動・回転・拡大縮小したり、アニメーションフォルダーでコマごとに描いたりできます(アニメーション → アニメーションツール)。
- **描画** — ペン・マーカー・エアブラシ・スプレーのブラシと筆圧、直線・図形、選択と変形、テキスト。
- **ドット絵** — ピクセルモードでくっきりしたドット絵を描き、大きく拡大して書き出せます。
- **タブ** — ひとつのウィンドウで複数の絵を切り替えながら作業できます。
- **自分好みに** — パネルの配置、ライト・ダークテーマ、ショートカットを変えられます。突然終了しても作業を復元します。

設定は **編集 → 設定**(`Ctrl+,`)にあります。

## よく使うショートカット

| キー | 操作 |
| --- | --- |
| `B` / `E` | ブラシ / 消しゴム |
| `P` | 再生 / 一時停止 |
| `Space` + ドラッグ、スクロール | キャンバスの移動、拡大・縮小 |
| `Ctrl+Z` / `Ctrl+Y` | 元に戻す / やり直す |
| `I` または `Alt` + クリック | 色を拾う |

ショートカットは **設定 → ショートカット** ですべて変更できます。

## 困ったときは

[GitHub の Issue](https://github.com/nyabi-gh/Ugurugu/issues) に、何をしていて何が
起きたかを書いてください。**設定 → 情報** の診断情報とログを添えていただくと、
早く直せます。

## クレジットとライセンス

- App icon artwork by seuppi

Copyright (C) 2026 Nyabi (nyabi-gh). [GPL-3.0-or-later](LICENSE) で配布しています。
ソースからのビルドは [BUILDING.md](BUILDING.md) をご覧ください。使用しているフォントと
ライブラリの通知は、アプリと一緒にインストールされる `THIRD_PARTY_NOTICES.txt` にあります。
