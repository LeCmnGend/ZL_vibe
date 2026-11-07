const fs = require('fs');`nconst os = require('os');
const path = require('path');
const zproto = require('./zproto_lib.js');

const WASM_PATH = path.resolve(__dirname, "../../asar_extracted/pc-dist/libs/libzproto_wasm_bg.ecc0555ba9d0f8867a4699ce943ec745.wasm");`n`nconst TEMP_DIR = os.tmpdir();`nconst KEYS_FILE = path.join(TEMP_DIR, "sync_v2_keys.json");`nconst SESSION_FILE = path.join(TEMP_DIR, "sync_v2_session.bin");`nconst MESSAGE_FILE = path.join(TEMP_DIR, "sync_v2_message.bin");

let wasmReady = false;

async function initWasm() {
    if (!wasmReady) {
        const wasmBuf = fs.readFileSync(WASM_PATH);
        await zproto.b(wasmBuf);
        wasmReady = true;
    }
}

async function generateV2Keys() {
    await initWasm();
    const ik = zproto.e();
    const ek = zproto.e();
    const res = {
        ik_pub: Buffer.from(ik.publicKey).toString('base64'),
        ik_priv: Buffer.from(ik.privateKey).toString('hex'),
        ek_pub: Buffer.from(ek.publicKey).toString('base64'),
        ek_priv: Buffer.from(ek.privateKey).toString('hex')
    };
    ik.free();
    ek.free();
    fs.writeFileSync(KEYS_FILE), JSON.stringify(res, null, 2));
    return res;
}

async function decryptMeta(metaFilePath) {
    await initWasm();
    if (!fs.existsSync(KEYS_FILE)) {
        throw new Error('sync_v2_keys.json not found');
    }
    const keys = JSON.parse(fs.readFileSync(KEYS_FILE), 'utf8'));
    let metaBytes = fs.readFileSync(metaFilePath);
    
    // Strip 4-byte uint32 LE length header if present
    if (metaBytes.length >= 4) {
        const len = metaBytes.readUInt32LE(0);
        if (len === metaBytes.length - 4) {
            metaBytes = metaBytes.slice(4);
        }
    }
    
    const ik_pub = Buffer.from(keys.ik_pub, 'base64');
    const ik_priv = Buffer.from(keys.ik_priv, 'hex');
    const ek_priv = Buffer.from(keys.ek_priv, 'hex');

    const res = zproto.j(metaBytes, ik_pub, ik_priv, ek_priv, null);
    const sessionRecord = Buffer.from(res.sessionRecord);
    const plaintext = Buffer.from(res.plaintext);
    res.free();

    fs.writeFileSync(SESSION_FILE, sessionRecord);
    return {
        sessionRecordHex: sessionRecord.toString('hex'),
        plainLen: plaintext.length
    };
}

async function decryptMessage(msgFilePath) {
    await initWasm();
    if (!fs.existsSync(SESSION_FILE)) {
        throw new Error('sync_v2_session.bin not found');
    }
    const sessionRecord = fs.readFileSync(SESSION_FILE);
    let msgBytes = fs.readFileSync(msgFilePath);

    // Strip 4-byte uint32 LE length header if present
    if (msgBytes.length >= 4) {
        const len = msgBytes.readUInt32LE(0);
        if (len === msgBytes.length - 4) {
            msgBytes = msgBytes.slice(4);
        }
    }

    const res = zproto.i(msgBytes, sessionRecord);
    const plaintext = Buffer.from(res.plaintext);
    res.free();

    fs.writeFileSync(MESSAGE_FILE, plaintext);
    return {
        plainLen: plaintext.length
    };
}

const cmd = process.argv[2];
if (cmd === 'gen-keys') {
    generateV2Keys().then(keys => {
        console.log(JSON.stringify(keys));
        process.exit(0);
    }).catch(e => {
        console.error(e);
        process.exit(1);
    });
} else if (cmd === 'decrypt-meta') {
    const metaPath = process.argv[3];
    decryptMeta(metaPath).then(res => {
        console.log(JSON.stringify(res));
        process.exit(0);
    }).catch(e => {
        console.error(e);
        process.exit(1);
    });
} else if (cmd === 'decrypt-message') {
    const msgPath = process.argv[3];
    decryptMessage(msgPath).then(res => {
        console.log(JSON.stringify(res));
        process.exit(0);
    }).catch(e => {
        console.error(e);
        process.exit(1);
    });
}

module.exports = { generateV2Keys, decryptMeta, decryptMessage, initWasm, zproto };
