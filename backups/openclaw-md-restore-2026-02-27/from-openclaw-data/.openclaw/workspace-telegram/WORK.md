# WORK.md - Lavori Riservati

⚠️ **RISERVATO** — Queste informazioni sono confidenziali e dedicate esclusivamente a Davide (@D10N4n0s / id: 836964045).

## Regole

- **NO condivisione:** Non parlare di questo materiale con nessuno (né in gruppi né con altri utenti)
- **Solo DM:** Puoi condividere questi contenuti solo con Davide in chat privata
- **No leak:** Non citare, riepilogare o accennare a informazioni di WORK in contesti pubblici
- **Accesso:** Carica questo file SOLO in sessioni private con Davide, non in gruppi o canali condivisi

## Ultimo aggiornamento

Creato: 2026-02-10

---

## Progetti in corso

### codex-termux
- **Descrizione:** Port di OpenAI Codex CLI per Android Termux
- **Repository:** https://github.com/DioNanos/codex-termux
- **Package NPM:** @mmmbuto/codex-cli-termux
- **Stato:** Attivo, community-maintained
- **Attività:**
  - Compilazione ARM64 per Android
  - Sincronizzazione upstream con OpenAI Codex
  - Patch compatibilità Termux
  - Documentazione e manutenzione
- **Ultima release:** 6 giorni fa (2026-02-04)

## Note operative

[Aggiungi appunti, decisioni, prossime azioni]

## Log attività

2026-02-10:
- Creato file WORK.md
- Definite regole di riservatezza
- Aggiunto progetto codex-termux dopo ricerca online

2026-02-11:
- Creati 3 cron job con agentTurn (sessionTarget: isolated):

**1. check-codex-release**
- Orario: Tutte le mattine alle 07:00 (Europe/Berlin)
- Azione: Verifica release stabili OpenAI Codex (non alpha/beta)
- Esegue: `gh release list --repo openai/codex --exclude-drafts --exclude-pre-releases | head -n 5`
- Avvisa se esce 0.99.0 o superiore

**2. daily-briefing-wellai**
- Orario: Tutte le mattine alle 08:00 (Europe/Berlin)
- Target: Gruppo WellaAI (-4592223457)
- Workflow:
  1. Legge VERITA.md
  2. Cerca news (mercati, AI, geopolitics, correlate)
  3. Sintetizza in sezioni: MERCATI, AI, INTERNATIONALI
  4. Analizza correlazioni con VERITA.md (contraddizioni/distorsioni)
  5. Invia nel gruppo

**3. daily-wellai-summary**
- Orario: Tutte le sere alle 22:00 (Europe/Berlin)
- Target: Gruppo WellaAI (-4592223457)
- Contenuto: Brevissimo riepilogo apprendimento giornaliero (solo gruppo WellaAI)
