# Remote MCP Gateway - Tasks

## 実装ロードマップ

| Phase | タスク | 依存 | ステータス |
|-------|--------|------|-----------|
| 1 | 既存 stdio MCP を Streamable HTTP 対応に変更 | - | TODO |
| 2 | ユーザーごとの OAuth トークン管理 | Phase 1 | TODO |
| 3 | Cloud Run にデプロイ | Phase 2 | TODO |
| 4 | Claude 管理コンソールで組織コネクタとして登録・動作確認 | Phase 3 | TODO |
| 5 | 権限制御 (YAML ホワイトリスト) | Phase 2 | TODO |
| 6 | tools/list のユーザー別フィルタ | Phase 5 | TODO |
| 7 | 利用統計 (Cloud Logging) | Phase 2 | TODO |

## Phase 詳細

### Phase 1: Streamable HTTP 対応

- [ ] HTTP サーバー追加（既存 `mcp_server.rs` の stdio 実装をベースに）
- [ ] MCP Streamable HTTP トランスポート実装
- [ ] ローカルで HTTP モードでの動作確認

### Phase 2: OAuth トークン管理

- [ ] Google OAuth フロー実装（認可エンドポイント、コールバック）
- [ ] ユーザー識別（Google email）
- [ ] トークンの暗号化保存
- [ ] トークンリフレッシュ処理

### Phase 3: Cloud Run デプロイ

- [ ] Dockerfile 作成
- [ ] CI/CD パイプライン構築
- [ ] Secret Manager 連携

### Phase 4: 組織コネクタ登録

- [ ] Claude 管理コンソールでの登録手順確認
- [ ] エンドツーエンド動作確認

### Phase 5: 権限制御

- [ ] YAML パーサー実装
- [ ] ワイルドカードマッチング実装
- [ ] リクエスト時の権限チェックミドルウェア
- [ ] 未登録ユーザーの拒否処理

### Phase 6: tools/list フィルタ

- [ ] ユーザー権限に基づく tools/list レスポンスフィルタ
- [ ] 未登録ユーザーへの空リスト返却

### Phase 7: 利用統計

- [ ] 構造化ログ出力（email, timestamp, method ID, result）
- [ ] Cloud Logging への連携確認
