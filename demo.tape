# GWS CLI — YouTube Short Demo (FAST iteration mode)
# Run: vhs demo.tape
#
# All cosmetic sleeps minimized. API sleeps kept for responses.
# Single line commands for reliability.

Output demo.mp4
Output demo.gif

Set Shell "bash"
Set FontSize 22
Set Width 1080
Set Height 1920
Set TypingSpeed 1ms
Set Padding 30
Set LineHeight 1.3

# ── Setup (hidden) ──
Hide
# Mock gemini CLI for deterministic demo
Type 'function gemini() { echo "Why do Java developers wear glasses? Because they don'"'"'t C#."; }' Enter
Type "export -f gemini" Enter
Type "export PATH=$PWD/target/release:$PWD/target/debug:$PATH" Enter
Type "set -e" Enter
Sleep 1s
Type `DEMO=$(gws drive files create --json '{"name":"gws-demo","mimeType":"application/vnd.google-apps.folder"}' | jq -r '.id')` Enter
Sleep 3s
Type `gws drive files create --json "{\"name\":\"meeting-notes.md\",\"mimeType\":\"text/markdown\",\"parents\":[\"$DEMO\"]}" > /dev/null` Enter
Sleep 2s
Type `gws drive files create --json "{\"name\":\"quarterly-report.csv\",\"mimeType\":\"text/csv\",\"parents\":[\"$DEMO\"]}" > /dev/null` Enter
Sleep 2s
Type `gws drive files create --json "{\"name\":\"project-roadmap.md\",\"mimeType\":\"text/markdown\",\"parents\":[\"$DEMO\"]}" > /dev/null` Enter
Sleep 2s
Type `Q="'$DEMO' in parents"` Enter
Sleep 500ms
Type "clear" Enter
Sleep 1s
Show

# ╔══════════════════════════════════════╗
# ║         ASCII ART INTRO             ║
# ╚══════════════════════════════════════╝

Hide
Type "./scripts/show-art.sh art/intro.txt" Enter
Sleep 1s
Show
Sleep 1s

# ── Scene 1: What is gws? ──
Hide
Type "./scripts/show-art.sh art/scene1.txt" Enter
Sleep 1s
Show
Sleep 4s

# ── Scene 2: Discover all services ──
Hide
Type "./scripts/show-art.sh art/scene2.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws --help 2>&1 | head -40" Enter
Sleep 4s

# ── Scene 2b: Inspect Drive resources ──
Hide
Type "./scripts/show-art.sh art/scene2b.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws drive --help 2>&1 | head -40" Enter
Sleep 4s

# ── Scene 3: Schema introspection ──
Hide
Type "./scripts/show-art.sh art/scene3.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws schema drive.files.list | head -15" Enter
Sleep 3s

# ── Scene 3b: Inspect JSON Schemas ──
Hide
Type "./scripts/show-art.sh art/scene3b.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws schema drive.File | head -20" Enter
Sleep 3s

Type "gws schema drive.File --resolve-refs | head -30" Enter
Sleep 5s

# ── Scene 4: List files in a folder ──
Hide
Type "./scripts/show-art.sh art/scene4.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws drive files list" 
Type ` --params "{\"q\":\"$Q\",\"fields\":\"files(name,mimeType)\"}"` Enter
Sleep 3s

# ── Scene 5: Upload a file ──
Hide
Type "./scripts/show-art.sh art/scene5.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "echo '# Notes' > /tmp/notes.md" Enter
Sleep 500ms

Type "gws drive files create" 
Type ` --json '{"name":"notes.md","mimeType":"text/markdown"}'` 
Type " --upload /tmp/notes.md" Enter
Sleep 3s

# ── Scene 6: Gmail labels ──
Hide
Type "./scripts/show-art.sh art/scene6.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "JOKE=$(gemini 'tell me a joke')" Enter
Sleep 1s

Type `MSG=$(echo -e "To: justin@example.com\nSubject: joke of the day\n\n$JOKE" | base64 | tr -d '\n' | tr '+/' '-_' | tr -d '=')` Enter

Type "gws gmail users messages send" 
Type ` --params '{"userId":"me"}'` 
Type ` --json "{\"raw\":\"$MSG\"}"` 
Type " | jq ." Enter
Sleep 3s

# ── Scene 7: Calendar event ──
Hide
Type "./scripts/show-art.sh art/scene7.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws calendar events insert" 
Type ` --params '{"calendarId":"primary"}'` 
Type ` --json '{"summary":"Ship v1.0 🚀","start":{"dateTime":"2024-06-17T10:00:00-07:00"},"end":{"dateTime":"2024-06-17T10:30:00-07:00"}}'` 
Type " | jq . | head -15" Enter
Sleep 3s

# ── Scene 8: Sheets automation ──
Hide
Type "./scripts/show-art.sh art/scene8.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws sheets spreadsheets values append" 
Type ` --params '{"spreadsheetId":"1izmtvgBC4NuxHhABFX6descuB6-SXTm3g7c6LYBngJQ","range":"Sheet1!A1","valueInputOption":"USER_ENTERED"}'` 
Type ` --json '{"values":[["Deploy","v1.0","=NOW()"]]}'` Enter
Sleep 3s

# ── Scene 9: Auto-pagination ──
Hide
Type "./scripts/show-art.sh art/scene9.txt" Enter
Sleep 1s
Show
Sleep 1s

Type "gws drive files list" 
Type " --params '" Sleep 50ms
Type@30ms '{"pageSize":2,"fields":"nextPageToken,files(id)"}' Sleep 50ms
Type "'" 
Type " --page-all" 
Type " | jq -r '.files[]?.id'" Enter
Sleep 6s

# ╔══════════════════════════════════════╗
# ║         OUTRO                        ║
# ╚══════════════════════════════════════╝

Hide
Type "./scripts/show-art.sh art/outro.txt" Enter
Sleep 1s
Show
Sleep 1s

# ── Cleanup (hidden) ──
Hide
Type `gws drive files delete --params "{\"fileId\":\"$DEMO\"}" > /dev/null 2>&1` Enter
Sleep 3s
