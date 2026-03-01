# BACKUP OpenClaw - 2026-02-20 18:07 CET

## Motivo Backup

Token WhatsApp Cloud API scaduto. Backup effettuato prima di test con token temporaneo.

## Ripristino

```bash
# Ripristina config
cp /home/dag/backup-openclaw-20260220-180736/secrets.env ~/.openclaw/
cp /home/dag/backup-openclaw-20260220-180736/openclaw.json ~/.openclaw/

# Poi aggiorna WA_ACCESS_TOKEN con nuovo token permanente
nano ~/.openclaw/secrets.env

# Riavvia gateway
systemctl --user restart openclaw-gateway.service
```

## Documentazione Completa

Vedi: `~/.docs/projects/openclaw/vps1/RIPRISTINO-PRODUZIONE.md`
