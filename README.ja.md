<div align="center">

<img src="resources/logo.svg" width="112" height="112" alt="Argus のロゴ">

# Argus

**高速でモダンな、情報豊富な Windows タスク マネージャー**

[简体中文](README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · [Español](README.es.md) · [Deutsch](README.de.md) · **日本語** · [Français](README.fr.md) · [Русский](README.ru.md)

[![公式サイトとデモ](https://img.shields.io/static/v1?label=%F0%9F%8C%90+%E5%85%AC%E5%BC%8F%E3%82%B5%E3%82%A4%E3%83%88%E3%81%A8%E3%83%87%E3%83%A2&message=argustask.com&color=2563eb&style=for-the-badge)](https://argustask.com/ja/)

[公式サイト](https://argustask.com/ja/)では、インストールせずにブラウザー上で画面を操作し、機能ガイドを体験できます。

[![最新バージョン](https://img.shields.io/github/v/release/Ray-lea/Argus?include_prereleases=&sort=semver&label=%E6%9C%80%E6%96%B0%E3%83%90%E3%83%BC%E3%82%B8%E3%83%A7%E3%83%B3&color=2f80ed)](https://github.com/Ray-lea/Argus/releases)
[![ダウンロード](https://img.shields.io/github/downloads/Ray-lea/Argus/total?label=%E3%83%80%E3%82%A6%E3%83%B3%E3%83%AD%E3%83%BC%E3%83%89&color=2ea44f)](https://github.com/Ray-lea/Argus/releases)
[![動作環境](https://img.shields.io/static/v1?label=%E5%8B%95%E4%BD%9C%E7%92%B0%E5%A2%83&message=Windows+10+%2F+11+x64&color=0078D4)](#system-requirements)
[![ライセンス](https://img.shields.io/static/v1?label=%E3%83%A9%E3%82%A4%E3%82%BB%E3%83%B3%E3%82%B9&message=AGPL-3.0-or-later&color=A42E2B&logo=gnu&logoColor=white)](LICENSE)

[機能](#features) · [ダウンロード](#download) · [ソースからのビルド](#build-from-source) · [ショートカット](#keyboard-shortcuts) · [ドキュメント](#documentation) · [コミュニティ](#community)

</div>

<br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/screenshot-en-dark.png">
  <img src="docs/images/screenshot-en-light.png" alt="アプリを名前順に並べた Argus のプロセス画面。各プロセスのリソース使用量と、直近 1 分間の CPU、メモリ、ディスク、ネットワーク、GPU のグラフを表示。">
</picture>

<sub>プレビュー画像は英語表示です。</sub>

<br>

Argus は Rust から Windows のネイティブ API を直接呼び出し、GPUI Kit のネイティブ コンポーネントで画面を構築しています。グラフはディスプレイのリフレッシュ レートに合わせて滑らかに動きます。名前は、ギリシャ神話に登場する百の目を持つ巨人アルゴス（Argus Panoptes）に由来します。

<a id="star-history"></a>

## ⭐ GitHub Star の推移

Argus が日常の作業に役立った際は、GitHub で Star を付けていただけると、プロジェクトの支援や認知拡大につながります。

<a href="https://www.star-history.com/#Ray-lea/Argus&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=Ray-lea/Argus&type=Date&theme=dark">
    <img src="https://api.star-history.com/svg?repos=Ray-lea/Argus&type=Date" alt="Argus の GitHub Star 数の推移">
  </picture>
</a>

<a id="features"></a>

## ✨ 機能

| | 機能 | 内容 |
|:---:|---|---|
| 🧮 | **プロセス** | アプリ別、プロセス ツリー、一覧の 3 種類の表示に対応。各プロセスの CPU、メモリ、ディスクの読み書き、ネットワーク通信量、GPU 使用率をリアルタイムで表示します。並べ替え、検索、プロセスやツリー全体の終了が可能で、詳細ペインではコマンド ライン、ユーザー、整合性レベルなどを確認できます。 |
| ⚡ | **優先度と効率モード** | 右クリック メニューから変更でき、プログラムの起動時に自動適用するルールも作成できます。 |
| 📈 | **パフォーマンス** | CPU のコア別負荷、P コアと E コア、周波数、温度、消費電力に加え、メモリ、ディスクの正常性と SMART 属性、ネットワーク、GPU、バッテリーを詳しく表示します。 |
| 🧩 | **サービスとスタートアップ** | Windows サービスの開始・停止とスタートアップの種類の変更、スタートアップ アプリの有効化・無効化に対応。状態は Windows タスク マネージャーと共通です。 |
| 🌐 | **接続とポート** | 使用中の TCP・UDP エンドポイントと関連するプロセスを表示し、ポート番号による完全一致検索や接続の切断ができます。 |
| 🔍 | **ファイルの使用状況** | ファイルやフォルダーをウィンドウにドロップすると、使用中のプロセスを検索します。Restart Manager とハンドル列挙を組み合わせ、プロセスの作業ディレクトリも調べます。 |
| 🕘 | **履歴** | 過去 7 日間のリソース使用量を自動記録します。グラフ上の時刻を選択すると、その時点で負荷の高かったプロセスを確認できます。 |
| 📌 | **通知領域への常駐** | グローバル ショートカットでウィンドウを表示・非表示にできます。非表示中は少ないリソースで動作し、最小化中と通知領域に常駐中の更新間隔を個別に設定できます。 |
| 🔄 | **アプリ内更新** | 安定版とベータ版の更新チャネルを提供。新しいバージョンをバックグラウンドでダウンロードし、タイトル バーで通知します。更新内容は更新前後に確認できます。既定のダウンロード経路は GitHub、公式ミラー、GitHub プロキシの順で接続し、設定から変更できます。 |
| 🎨 | **ネイティブな外観** | Windows 11 では Mica 背景、Windows 10 では単色背景を使用します。アクセント カラーとライト・ダーク テーマは Windows に従い、テーマは設定から個別に選択できます。 |
| 🌍 | **多言語対応** | 簡体字中国語、繁体字中国語、英語、スペイン語、ドイツ語、日本語、フランス語、ロシア語に対応。初期設定では Windows の表示言語に従います。初回設定ガイドから基本設定を行えます。 |
| 👤 | **管理者権限なしで利用可能** | 標準ユーザー権限で、全プロセスの実行ファイルのパスとリソース使用量を確認できます。 |
| 🔐 | **管理者としての起動を選択可能** | 設定または初回設定ガイドで一度承認すると、サインイン時、ショートカット、タスク マネージャーの起動操作から管理者として実行できます。いつでも無効にできます。 |
| 🛡️ | **任意の特権サービス** | 管理者の承認を受けてインストールすると、画面を標準権限で実行したまま、システム プロセスの管理、高い権限のプロセスによるファイル使用状況の確認、ルールの自動適用、サービス操作、接続の切断、プロセス別通信量の取得が可能になります。Windows 10 と Windows 11 24H2 より前のバージョンでは、プロセス別のディスク読み書きも取得します。設定からいつでもアンインストールできます。 |
| 🪟 | **タスクとシステム連携** | [Windows タスク マネージャーの置き換え](docs/architecture/task-manager.md)（説明は中国語）、任意のページからの新しいタスクの実行、管理者としての実行、現在のデスクトップのエクスプローラーの再起動に対応します。 |

<a id="download"></a>

## 📦 ダウンロード

最新の安定版と SHA-256 チェックサムは、[公式ダウンロード ページ](https://argustask.com/ja/download/)から取得できます。すべてのバージョンは [GitHub Releases](https://github.com/Ray-lea/Argus/releases) にあります。

| ファイル | 内容 |
|---|---|
| `Argus_<version>_x64-setup.exe` | **インストーラー版（推奨）。** 全ユーザー向けに `Program Files` へインストールします。インストールと更新には管理者の承認が必要です。アプリ内更新に対応します。 |
| `Argus_<version>_x64.exe` | **ポータブル版。** 単一の実行ファイルから起動し、設定と履歴をインストーラー版と共有します。自動更新には対応していません。 |
| `SHA256SUMS.txt` | 配布ファイルの SHA-256 チェックサム。 |

[コード署名ポリシー](#code-signing-policy) · [プライバシー ポリシー（英語）](PRIVACY.md#english)

<a id="system-requirements"></a>

## 💻 動作環境

- **OS**：Windows 10 22H2（ビルド 19045）以降、Windows 11 を含む。x64 のみ対応。
- **グラフィックス**：DirectX 11 に対応したグラフィックス デバイスとドライバー。

確認済みの Windows バージョン、機能差、権限要件は、[互換性の説明](docs/guide/hardware-data.md#系统兼容范围)（中国語）を参照してください。

<a id="build-from-source"></a>

## 🔧 ソースからのビルド

必要な環境：

- Rust `stable-x86_64-pc-windows-msvc` ツールチェーン（1.99.1 以降）。
- Node.js 26.12.1 以降の 26.x、または 24.22.1 以降の 24.x LTS、および pnpm 12.14.0。
- Visual Studio Build Tools の「C++ によるデスクトップ開発」ワークロード。

リポジトリのルート ディレクトリで PowerShell から実行します。

```powershell
.\start.bat           # 開発モード：Rust とリソースの変更時に再ビルド・再起動。失敗時は既存のインスタンスを維持
.\start.bat build     # リリース ビルド：target\release\argus.exe を生成
```

コードのチェック、インストーラーの作成、キャッシュの削除は、[開発ガイド](docs/development/README.md)（中国語）を参照してください。

<a id="keyboard-shortcuts"></a>

## 🔑 キーボード ショートカット

| 操作 | キー |
|---|---|
| ページの切り替え | <kbd>Ctrl</kbd>+<kbd>1</kbd> … <kbd>Ctrl</kbd>+<kbd>8</kbd> |
| プロセスの検索 | <kbd>Ctrl</kbd>+<kbd>F</kbd> |
| 更新の一時停止 | <kbd>Ctrl</kbd> を押している間は停止し、離すと再開（プロセス・パフォーマンス ページ） |
| 詳細を開く | <kbd>Enter</kbd> |
| タスクの終了 | <kbd>Alt</kbd>+<kbd>E</kbd> または <kbd>Delete</kbd>。<kbd>Shift</kbd>+<kbd>Delete</kbd> は確認を省略 |
| ウィンドウの表示・非表示（グローバル） | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Esc</kbd> |

全一覧は **設定 → キーボード ショートカット → ショートカットを表示** から確認できます。詳しい操作は、[利用ガイド](docs/guide/README.md)（中国語）を参照してください。

<a id="documentation"></a>

## 📚 ドキュメント

詳細ドキュメントは中国語です。[オンライン ドキュメント](https://argustask.com/docs/)では、安定版と開発版の説明を閲覧できます。

| ドキュメント | 内容 |
|---|---|
| [利用ガイド](docs/guide/README.md) | 画面と操作、通知領域とショートカット、設定、ハードウェア データの取得元と制限 |
| [開発ガイド](docs/development/README.md) | 環境とコマンド、ビルドとキャッシュ、性能の基準と測定方法 |
| [アーキテクチャ](docs/architecture/README.md) | システム構成、データ フロー、技術構成、各モジュールの役割 |
| [設計](docs/design/README.md) | 設計方針、グラフのアニメーション、ウィンドウの外観とマテリアル |
| [既知の問題](docs/known-issues.md) | 現行バージョンの制限事項 |
| [更新履歴](changelog/ja-JP.md) | 各バージョンの詳細な変更内容 |

<a id="community"></a>

## 💬 コミュニティ

[![QQ グループ 453803497](https://img.shields.io/badge/QQ-453803497-12B7F5?logo=qq&logoColor=white)](https://qm.qq.com/q/97YqJAQRNK)

中国語の QQ グループで、不具合の報告、機能の提案、利用者同士の交流ができます。Argus の **バージョン情報 → QQ グループに参加** からも開けます。

<a id="contributing"></a>

## 🤝 貢献

Issue や Pull Request を歓迎します。[貢献ガイド](.github/CONTRIBUTING.md)（中国語）を確認してから参加してください。セキュリティ上の問題は、[セキュリティ ポリシー](.github/SECURITY.md)（中国語）に従って非公開で報告してください。

<a id="roadmap"></a>

## 🧭 ロードマップ

完了した項目と全体計画は、[ロードマップ](docs/roadmap.md)（中国語）にまとめています。現在進行中、または予定している主な項目は次のとおりです。

- [ ] **ハードウェア センサーの拡充**：公式 PawnIO ドライバーを利用して、コンシューマー向けハードウェアの対応範囲を拡大。[現在の対応範囲](docs/guide/hardware-data.md)（中国語）を参照。
- [ ] **ショートカットの拡張**：左右の修飾キーの区別、修飾キーのみの組み合わせ、通常キー 2 つの組み合わせに対応。

<a id="code-signing-policy"></a>

## 🔐 コード署名ポリシー

無料のコード署名サービスは [SignPath.io](https://about.signpath.io/)、証明書は [SignPath Foundation](https://signpath.org/) から提供されています。

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

テスト署名は導入済みです。現在の正式リリースには Windows Authenticode 署名がありません。正式署名の有効化後は、このリポジトリのソースから GitHub Actions でビルドする Argus 本体、タスク マネージャー補助プログラム、アンインストーラー、インストーラーが対象になります。同梱する第三者のプログラムは元の署名を維持します。

- **コードの管理とレビュー**：[@Ray-lea](https://github.com/Ray-lea)。コードの保守と外部からの貢献のレビューを担当します。
- **署名の承認**：[@Ray-lea](https://github.com/Ray-lea)。正式署名のリクエストを毎回手動で承認します。
- **プライバシー ポリシー**：[English](PRIVACY.md#english) · [简体中文](PRIVACY.md#简体中文)。既定で有効な更新確認、プロキシへの切り替え、必要時の PawnIO ダウンロード、任意のフィードバック、第三者によるデータの取り扱いを説明しています。

アプリ内更新では、独立した Minisign リリース キーでインストーラーとバージョン番号を検証します。Windows Authenticode は実行ファイルの署名者を確認するための仕組みです。

<a id="acknowledgements"></a>

## 🙏 謝辞

Argus は次のオープンソース プロジェクトを利用しています。

| プロジェクト | 用途 |
|---|---|
| [GPUI Kit](https://gpui-kit.com/) | ネイティブなデスクトップ部品、ウィンドウ、表、グラフ |
| [windows-rs](https://github.com/microsoft/windows-rs) | Windows ネイティブ API の Rust バインディング |
| [Biome](https://biomejs.dev/)、[NSIS](https://nsis.sourceforge.io/) | 開発時のチェックと Windows 向けパッケージ作成 |
| [serde](https://serde.rs/)、[reqwest](https://github.com/seanmonstar/reqwest) などの Rust crate | シリアル化、ネットワーク通信などの基盤機能 |
| [PawnIO](https://pawnio.eu/)、[PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) | 任意のカーネル センサーに使う署名済みドライバーとデータ収集モジュール |
| [ZenStates-Core](https://github.com/irusanov/ZenStates-Core)、[LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)、[GNR-SMU](https://github.com/Kyworn/gnr-smu) | AMD SMU テーブルなど、CPU レジスター構成の参考資料 |
| [Intel IGCL](https://github.com/intel/drivers.gpu.control-library)、[AMD ADL](https://github.com/GPUOpen-LibrariesAndSDKs/display-library) | GPU テレメトリ インターフェース |

第三者コンポーネントとライセンス表記の全一覧は [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt) にあります。

[LINUX DO](https://linux.do/) コミュニティの支援に感謝します。

<a id="license"></a>

## 📄 ライセンス

Argus は、[GNU Affero General Public License](LICENSE) バージョン 3、または任意のそれ以降のバージョン（`AGPL-3.0-or-later`）で公開する自由ソフトウェアです。

<a href="https://next.ossinsight.io/widgets/official/compose-recent-active-contributors?repo_id=1415551131&limit=30" target="_blank" style="display: block" align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://next.ossinsight.io/widgets/official/compose-recent-active-contributors/thumbnail.png?repo_id=1415551131&limit=30&image_size=auto&color_scheme=dark" width="655" height="auto">
    <img alt="Active Contributors of Ray-lea/Argus - Last 28 days" src="https://next.ossinsight.io/widgets/official/compose-recent-active-contributors/thumbnail.png?repo_id=1415551131&limit=30&image_size=auto&color_scheme=light" width="655" height="auto">
  </picture>
</a>

<!-- Made with [OSS Insight](https://ossinsight.io/) -->

<div align="center">
<br>
<sub>Argus · Windows 用タスク マネージャー · © 2026 Argus contributors</sub>
</div>
