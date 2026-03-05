# Remote MCP Gateway - Requirements

## 背景

会社で契約している Claude Desktop / Claude Code から Google Workspace API を安全に利用したい。
gws を Claude for Business/Enterprise の組織コネクタ（Integrations）として全社配布する。

## ユーザー

- 社員約20名
- 管理者ユーザー: 権限設定を管理する
- 通常ユーザー: 許可された操作のみ実行する

## 要件

### R1: 組織コネクタとしての配布

- Claude 管理コンソールから組織コネクタとして登録し、全メンバーに自動配布する
- メンバーは Claude Desktop / Claude.ai 上でコネクタを利用開始できる

### R2: Google アカウントによる認証

- 各メンバーが自分の Google アカウントで OAuth し、MCP 経由で Google Workspace を操作する
- MCP Gateway への認証と Google API への認可を 1 回の Google OAuth ログインで兼用する

### R3: ホワイトリスト方式の権限制御

- 管理者が通常ユーザーに対して、実行可能な操作をホワイトリスト方式で設定する
- 権限の粒度は Google Discovery JSON のメソッド ID 単位（例: `gmail.users.messages.send`）
- 未登録ユーザーは何もできない
- MCP の `tools/list` はユーザーに許可されたメソッドのみ返す（エージェントが不許可操作を試みること自体を防ぐ）

### R4: 利用統計

- 各ユーザーの利用状況（誰が・いつ・どの操作を・何回呼んだか）を記録する
- 目的は活用度の把握

## 非機能要件

- GCP 上でホストする
- 20名規模に対応できればよい（大規模スケーリングは不要）
