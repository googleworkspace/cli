# Fork: MCP サーバー機能を温存した gws

このリポジトリは [googleworkspace/cli](https://github.com/googleworkspace/cli) のフォークです。

[English version](FORK.md)

upstream が削除した **MCP（Model Context Protocol）サーバー機能** を独自にメンテナンスし、AI エージェントから Google Workspace API を直接呼び出せる状態を維持しています。

## upstream との差分

| 項目 | upstream | このフォーク |
|---|---|---|
| MCP サーバー (`gws mcp`) | 削除済み | 維持・メンテナンス中 |
| MCP helper tools (`--helpers`) | なし | `gmail_send` 等を独自実装 |
| CI/CD ワークフロー | upstream 環境依存 | 最小構成（CI + Policy + Sync + Release） |

### MCP サーバー

Discovery Document から動的にツールを生成し、stdio 経由で MCP プロトコルを提供します。

```bash
# Gmail の MCP サーバーを起動（helper tool 付き）
gws mcp -s gmail --helpers

# 複数サービスを同時に提供
gws mcp -s gmail -s drive -s calendar --helpers

# compact モード（サービスごとに1ツール）
gws mcp -s gmail --tool-mode compact
```

### MCP helper tools

`--helpers` フラグで有効化される便利ツールです。Discovery API の raw tool に加え、RFC 2822 構築や base64 エンコード等の面倒な処理を自動化します。

| ツール名 | 説明 |
|---|---|
| `gmail_send` | メール送信。to/subject/body を渡すだけで RFC 2822 フォーマット・base64url エンコードを自動処理 |
| `gmail_reply` | スレッド内返信。message_id/body を渡すだけで In-Reply-To, References, Re: 件名, threadId を自動設定 |

## インストール

### Homebrew（macOS / Linux）— 推奨

```bash
brew install shigechika/tap/gws-mcp
```

Rust ツールチェーン不要。macOS（Apple Silicon / Intel）と Linux（x86\_64 / arm64）向けのバイナリを事前ビルドして配布しています。

### Cargo（ソースからビルド）

```bash
# GitHub から直接インストール
cargo install --git https://github.com/shigechika/gws-mcp --locked
```

ローカルに clone 済みの場合は、ワーキングツリーからインストール:

```bash
cd gws-mcp
cargo install --path crates/google-workspace-cli
```

`~/.cargo/bin/gws` にバイナリがインストールされます。`cargo build --release` は `target/release/gws` にビルドするだけで `~/.cargo/bin/` は**更新されない**点に注意してください。

## Claude での使い方

**Claude Code** — `~/.claude.json` に追加:

```json
{
  "mcpServers": {
    "gws": {
      "command": "gws",
      "args": ["mcp", "-s", "gmail", "-s", "drive", "-s", "calendar", "--helpers"]
    }
  }
}
```

**Claude Desktop** — `~/Library/Application Support/Claude/claude_desktop_config.json`（macOS）に追加:

```json
{
  "mcpServers": {
    "gws": {
      "command": "gws",
      "args": ["mcp", "-s", "gmail", "-s", "drive", "-s", "calendar", "--helpers"]
    }
  }
}
```

## このフォークで対応した upstream の MCP issue

upstream の MCP サーバーに対するバグ報告・機能要望（MCP 削除に伴い close されたもの）を、このフォークで移植・対応しています。

| upstream issue | 状態 | 内容 |
|---|---|---|
| [#162](https://github.com/googleworkspace/cli/issues/162) — `tools/list` が呼び出せないツール名を返す（alias と doc.name の不一致） | 対応済 | `walk_resources` がツール名プレフィックスに Discovery doc 名ではなく設定された alias を使うよう変更。`tools/list` と `tools/call` の名前空間を統一 |
| [#170](https://github.com/googleworkspace/cli/issues/170) — 複数単語のリソース名（`admin_role_assignments_list` 等）でパースが壊れる | 対応済 | `split('_')` を Discovery ツリーに対する貪欲リゾルバ（`resolve_tool_path`）に置換。アンダースコアを含むリソース名・任意の入れ子に対応 |
| [#212](https://github.com/googleworkspace/cli/issues/212) — Full mode の schema が GET メソッドにも `body`/`upload` を含む | 対応済 | `method.request.is_some()` の時のみ `body` を、`supports_media_upload == true` の時のみ `upload` を付与 |
| [#251](https://github.com/googleworkspace/cli/issues/251) — `--upload` が絶対パス・トラバーサルパスを受理する | 対応済 | MCP の `upload` 引数で絶対パス・`..` 要素を拒否 |
| [#260](https://github.com/googleworkspace/cli/issues/260) — tool annotations（`readOnlyHint` / `destructiveHint` / `idempotentHint`） | 部分対応 | HTTP method から導出した annotations を全ツールに付与。`tool_search` メタツールとページネーションは未移植 |
| [#642](https://github.com/googleworkspace/cli/issues/642) — `parse_message_headers` の case-sensitive マッチが `CC` 等の非正規ケースのヘッダを落とす | 対応済 | ヘッダ名を小文字化してからマッチするよう変更。Exchange/Outlook 由来の `"CC"` 等、RFC 5322 §1.2.2 に沿った任意ケーシングを認識 |
| [#573](https://github.com/googleworkspace/cli/issues/573) — `gmail.users.messages.get` で `metadataHeaders` 配列がクエリパラメータに展開されない | 対応済 | Discovery パーサが `repeated: true` を保持（`discovery.rs`）し、JSON 配列値を複数クエリに展開する実装が入っている（`executor.rs`）。Discovery 駆動の MCP ツールも同じ挙動を継承 |
| [#625](https://github.com/googleworkspace/cli/issues/625) — `script` service が `services.rs` に未登録で helper が到達不能 | 対応済 | `ServiceEntry { aliases: &["script"], api_name: "script", version: "v1", ... }` として登録済み。`gws script ...` と MCP `script_*` ツールが正常に解決する |
| [#717](https://github.com/googleworkspace/cli/issues/717) — `gws auth status` が非 JSON を stdout に出力し `jq` パイプラインを破壊 | 対応済 | `Using keyring backend: <name>` は `credential_store.rs` で `eprintln!`（stderr）に出力される。`gws auth status \| jq .` は正常に動作 |
| [#562](https://github.com/googleworkspace/cli/issues/562) — 対話 TUI が `cloud-platform` スコープを無条件に注入し、Workspace の admin policy で制限される組織では login が失敗する | 対応済 | `run_discovery_scope_picker` の選択後 auto-inject を削除（`auth_commands.rs`）。`cloud-platform` が必要な用途（modelarmor 等）は picker で明示選択するか `--full` / `--scopes` で指定する |
| [#644](https://github.com/googleworkspace/cli/issues/644) — `gmail +send` が `userinfo.profile` スコープを付与済みでも「grant profile scope」ヒントを出し、From の表示名が null になる | 対応済 | `helpers/gmail/mod.rs` の表示名取得を People API (`/people/me?personFields=names`) から OIDC userinfo endpoint (`openidconnect.googleapis.com/v1/userinfo`) に変更。同じスコープで Workspace / 個人 Gmail どちらでも一貫したレスポンスが得られる。401/403 時のフォールバックメッセージも、一時的な拒否をスコープ欠落と誤診断しない表現に改訂 |

## upstream MCP 定点観測

| 時期 | 出来事 |
|---|---|
| 2026-03-04 | `feat: add gws mcp server` — upstream に MCP サーバーが追加 |
| 2026-03-05 | ブランチ `fix/mcp-hyphen-tool-names` が upstream に出現 — ツール名の区切り文字をアンダースコアからハイフンに変更 |
| 2026-03-06 | `fix!: Remove MCP server mode` — 追加からわずか2日で upstream が breaking change として MCP サーバーを削除 |
| 2026-03-06 | 同ブランチがマージされずに削除 — upstream での MCP 復活は見送り |

## upstream 同期方針

- 毎週月曜に GitHub Actions で upstream/main を自動マージ
- コンフリクト発生時は PR を作成して手動解決
- MCP 関連コード（`src/mcp_server.rs`、`pub(crate)` 可視性）の温存を最優先
- upstream のコミットメッセージから `#番号` 参照を除去（クロスリファレンス防止）
