# TOOLS.md - WhatsApp Skills & Permissions

## Skill Disponibili

### ✅ Skill Attive (Ready)

| Skill | Scopo | Accesso |
|-------|-------|---------|
| **chutes-image-gen** | Generazione/modifica immagini | Davide + Marco |
| **weather** | Meteo e previsioni | Davide + Marco |
| **session-logs** | Analisi log sessioni | Davide + Marco |
| **openai-whisper** | Speech-to-text locale | Davide + Marco |
| **skill-creator** | Creazione skill | Davide |
| **github** | GitHub CLI (gh) | Davide |
| **coding-agent** | Codex/Claude Code | Davide |
| **bluebubbles** | Plugin iMessage | Davide |
| **tmux** | Remote-control tmux | Davide |
| **video-frames** | Estrazione frame video | Davide |

### ⚠️ Skill Non Disponibili (Missing)

Queste skill risultano installate ma mancano le dipendenze:

- `gemini` → Manca Gemini CLI installato
- `1password` → Manca `op` CLI
- Altre skill bundled (vedi lista completa con `openclaw skills list`)

---

## Autorizzazioni per Utente

### **Davide** (+393471443005) - Owner
- ✅ **Accesso completo** a tutte le skill
- ✅ Può installare nuove skill
- ✅ Può modificare configurazioni
- ✅ Elevated mode abilitato
- ✅ Può modificare SOUL.md / USER.md / AGENTS.md

### **Marco** (+393924912119) - Trusted Collaborator
- ✅ **Skill accessibili:**
  - `chutes-image-gen` (generazione immagini)
  - `weather` (meteo)
  - `session-logs` (lettura log)
  - `openai-whisper` (trascrizione audio)
- ❌ **NON può:**
  - Installare nuove skill
  - Modificare configurazioni
  - Usare elevated mode
  - Accedere a skill sensibili:
    - `github` (write operations)
    - `coding-agent` (esecuzione codice)
    - `skill-creator` (creazione skill)
    - `bluebubbles` (iMessage)
    - `tmux` (sessioni remote)
    - `video-frames` (elaborazione video)

---

## Chutes Image Gen - Dettagli

**Skill:** `chutes-image-gen`
**Modello:** Qwen-Image-2512 (Chutes.ai)
**Endpoint:** https://image.chutes.ai/generate

### Parametri Default (WhatsApp)
- Dimensione: 1024x1024
- Steps: 50
- Guidance: 7.5
- Modello: Qwen-Image-2512
- **Batch max:** 3 immagini (WhatsApp: meno spam)

### Comandi
```bash
# Generazione singola
python3 ~/.openclaw/workspace/skills/chutes-image-gen/scripts/generate.py \
  --prompt "tramonto sulle montagne"

# Batch (3 immagini max su WhatsApp)
python3 ~/.openclaw/workspace/skills/chutes-image-gen/scripts/generate.py \
  --batch 3

# Alta qualità
python3 ~/.openclaw/workspace/skills/chutes-image-gen/scripts/generate.py \
  --prompt "paesaggio fantasy" --steps 70 --guidance 8.5

# Editing
python3 ~/.openclaw/workspace/skills/chutes-image-gen/scripts/edit.py \
  --prompt "aggiungi arcobaleno" --image /path/to/image.png
```

### Output
- Directory: `./tmp/chutes-images/YYYY-MM-DD-HH-MM-S/`
- File: `YYYYMMDD_HHMMSS_<slug>.png`
- Metadata: `prompts.json` (batch mode)

---

## Operazioni Riservate (Solo Davide)

- Installare nuove skill (`clawhub install`, `npx openclawskill install`)
- Modificare `openclaw.json`
- Usare `/elevated full` (esecuzione host senza approvazione)
- Accedere a `1password` (se installato)
- Push su GitHub, creare PR, release
- Modificare file di configurazione:
  - SOUL.md
  - USER.md
  - AGENTS.md
  - openclaw.json

---

## Note Sicurezza

1. **Sandbox**: Attiva per sessioni non-main (`mode: non-main`)
   - DM diretti: esecuzione su host (trusted)
   - Gruppi: esecuzione in Docker sandbox
2. **Elevated**: Solo Davide può usare `/elevated full`
3. **Skill install**: Solo Davide può installare skill nuove
4. **API Keys**: Tutte in `~/.openclaw/secrets.env` (mai commitare)
5. **WhatsApp Policy**:
   - DM: solo Davide e Marco (allowlist)
   - Gruppi: allowlist, silence by default

---

## Workspace Memory

- **Directory:** `/home/dag/.openclaw/workspace-whatsapp/memory/`
- **Contatti:** `contacts/*.md`
- **Gruppi:** `groups/*.md`
- **Log giornalieri:** `memory/YYYY-MM-DD.md`
