# Remote MCP Gateway - Design

## アーキテクチャ概要

```
┌─────────────────────────┐
│  Claude Desktop /       │
│  Claude Code            │
│  (各メンバーの端末)       │
└───────────┬─────────────┘
            │ MCP (Streamable HTTP)
            │ + セッショントークン
            ▼
┌───────────────────────────────────┐
│  MCP Gateway (Cloud Run)          │
│                                   │
│  ┌─────────┐  ┌───────────────┐  │
│  │ 認証層   │  │ 権限制御層     │  │
│  │ OAuth2  │  │ メソッドID     │  │
│  │ Google  │  │ ホワイトリスト  │  │
│  └────┬────┘  └───────┬───────┘  │
│       │               │          │
│  ┌────▼───────────────▼───────┐  │
│  │ 実行層                      │  │
│  │ gws コア (Discovery JSON    │  │
│  │ → API 呼び出し)             │  │
│  └────────────┬───────────────┘  │
│               │                  │
│  ┌────────────▼───────────────┐  │
│  │ ログ層                      │  │
│  │ Cloud Logging              │  │
│  └────────────────────────────┘  │
└───────────────────────────────────┘
            │
            ▼
    Google Workspace API
```

## 決定事項

| 項目 | 決定 | 理由 |
|------|------|------|
| MCP トランスポート | Streamable HTTP | 組織コネクタに必要 |
| 認証方式 | MCP OAuth フロー + Google OAuth 兼用 | 1回のログインで MCP 認証と Google API 認可を両立 |
| 権限粒度 | Discovery JSON メソッド ID 単位 | Google 公式の命名体系をそのまま利用でき、自前定義不要 |
| 権限デフォルト | 未登録ユーザーは何もできない | セキュリティ優先 |
| tools/list | ユーザー権限でフィルタ | エージェントの不要な試行を防止 |
| OAuth トークン保管 | サーバー側保持 | 組織コネクタの仕組み上クライアント送信は不可 |
| 権限設定 | Git リポジトリ内 YAML | PR レビューで変更管理、DB 不要 |
| 管理方法 | YAML 編集 → PR → マージ → 再デプロイ | エンジニア向け、変更履歴が残る |
| デプロイ先 | Cloud Run (GCP) | コンテナ化、スケーリング、IAM 連携 |
| 利用統計 | Cloud Logging | 20名規模なら十分 |
| OAuth スコープ | 全サービス一括要求 | 権限制御は自前レイヤーで絞る |

## 認証フロー

```
1. ユーザーが Claude 上でコネクタを初回利用
2. Claude が MCP Gateway の認可エンドポイントにリダイレクト
3. MCP Gateway が Google OAuth 同意画面を表示
4. ユーザーが自分の Google アカウントで同意
5. MCP Gateway が Google OAuth トークンをサーバー側に保存
6. MCP Gateway がセッショントークンを発行し Claude に返す
7. 以降、Claude はセッショントークンを MCP リクエストに付与
8. MCP Gateway はセッショントークンからユーザーを特定し、
   保存済みの Google OAuth トークンで API を呼び出す
```

## 権限制御

### メソッド ID

Discovery JSON で Google が公式に定義する一意識別子。

| メソッド ID | 操作内容 |
|-------------|----------|
| `gmail.users.messages.list` | メール一覧取得 |
| `gmail.users.messages.get` | メール取得 |
| `gmail.users.messages.send` | メール送信 |
| `drive.files.list` | ファイル一覧取得 |
| `drive.files.get` | ファイル取得 |
| `drive.files.create` | ファイル作成 |

### ワイルドカード

- `*` — 全メソッド許可
- `gmail.*` — Gmail の全メソッド許可
- `gmail.users.messages.*` — Gmail メッセージ関連の全メソッド許可

### 権限設定ファイル

```yaml
# config/permissions.yaml

roles:
  admin:
    allow:
      - "*"

  workspace-reader:
    allow:
      - "gmail.users.messages.list"
      - "gmail.users.messages.get"
      - "gmail.users.labels.list"
      - "drive.files.list"
      - "drive.files.get"
      - "calendar.events.list"
      - "calendar.events.get"

  gmail-full:
    allow:
      - "gmail.*"

users:
  admin@company.com:
    role: admin

  tanaka@company.com:
    role: workspace-reader

  suzuki@company.com:
    role: gmail-full
```

### リクエスト処理フロー

```
1. Claude から MCP リクエスト受信
2. セッショントークンからユーザーを特定
3. リクエストされたメソッド ID を権限設定と照合
4. 許可されていれば実行、拒否ならエラーを返す
```

## データストア

| データ | 保存先 | 備考 |
|--------|--------|------|
| 権限設定 (YAML) | Git リポジトリ → コンテナイメージに同梱 | PR レビューで変更管理 |
| OAuth トークン | Secret Manager または暗号化ストレージ | 機密情報 |
| 利用ログ | Cloud Logging | 構造化ログ |

## 利用統計

### 記録項目

| 項目 | 例 |
|------|-----|
| ユーザー email | tanaka@company.com |
| タイムスタンプ | 2026-03-05T10:30:00Z |
| メソッド ID | gmail.users.messages.list |
| 成功/失敗 | success |

### 基盤

Cloud Logging に構造化ログとして出力。必要に応じて BigQuery にエクスポートし可視化。
