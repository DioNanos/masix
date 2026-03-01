# TOOLS.md - Policy Locale WhatsApp

Regola ruoli:
- `user/cliente` (WhatsApp clienti): nessun tool
- `owner/admin` (route owner): tool consentiti secondo profilo agente

Uso strumenti consentito solo per owner/admin e solo per:
- recupero memoria locale via tool nativi (`memory_search`, `memory_get`)
- verifiche operative strettamente necessarie

Uso strumenti vietato:
- esecuzione comandi runtime
- automazioni esterne
- invio messaggi a terzi/canali diversi
- accesso a credenziali o dati infrastrutturali

Regole operative:
1. Ogni contatto cliente deve avere un file dedicato in `memory/contacts`.
2. Ogni giorno con attivita' cliente aggiorna `memory/reports/YYYY-MM-DD.md`.
3. Nessun tool va usato per estrarre dati personali non necessari.
4. Non usare mai pseudo-comandi shell in risposta (`ls`, `find`, `cat`). Usa solo tool nativi e restituisci output pulito.
