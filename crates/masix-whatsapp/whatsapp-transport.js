/* eslint-disable no-console */
const crypto = require('crypto');
const qrcode = require('qrcode-terminal');
const { Client, LocalAuth } = require('whatsapp-web.js');

const SCHEMA_VERSION = 'whatsapp.v1';
const SHARED_SECRET = process.env.MASIX_WA_INGRESS_SECRET || '';

function computeSignature(schemaVersion, from, text, ts) {
  if (!SHARED_SECRET) {
    return null;
  }
  const payload = `${schemaVersion}\n${from}\n${text}\n${ts || 0}`;
  return crypto.createHmac('sha256', SHARED_SECRET).update(payload).digest('base64');
}

const client = new Client({
  authStrategy: new LocalAuth(),
});

client.on('qr', (qr) => {
  qrcode.generate(qr, { small: true });
  console.error('Scan QR code above to login');
});

client.on('ready', () => {
  console.error('WhatsApp client ready (read-only mode)');
});

client.on('message', async (message) => {
  const from = message.from || '';
  const text = message.body || '';
  const ts = message.timestamp ? Number(message.timestamp) : null;

  if (!from || !text) {
    return;
  }

  const envelope = {
    schema_version: SCHEMA_VERSION,
    from,
    text,
    ts,
    signature: computeSignature(SCHEMA_VERSION, from, text, ts),
    meta: {
      hasMedia: Boolean(message.hasMedia),
      type: message.type || 'chat',
    },
  };

  console.log(JSON.stringify(envelope));
});

client.initialize();
