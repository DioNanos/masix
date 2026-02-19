const qrcode = require('qrcode-terminal');
const { Client, LocalAuth } = require('whatsapp-web.js');

const client = new Client({
    authStrategy: new LocalAuth()
});

client.on('qr', (qr) => {
    qrcode.generate(qr, { small: true });
    console.error('Scan QR code above to login');
});

client.on('ready', () => {
    console.error('WhatsApp client ready');
});

client.on('message', async (message) => {
    const envelope = {
        id: crypto.randomUUID(),
        channel: 'whatsapp',
        kind: {
            type: 'message',
            from: message.from,
            text: message.body
        },
        payload: {}
    };
    
    console.log(JSON.stringify(envelope));
});

client.initialize();

// Handle commands from stdin
const readline = require('readline');
const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
});

rl.on('line', async (line) => {
    try {
        const cmd = JSON.parse(line);
        
        if (cmd.action === 'send') {
            await client.sendMessage(cmd.to, cmd.text);
            console.error(`Sent message to ${cmd.to}`);
        }
    } catch (err) {
        console.error('Error processing command:', err);
    }
});
