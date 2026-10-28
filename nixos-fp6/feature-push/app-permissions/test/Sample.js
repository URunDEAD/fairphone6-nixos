// A state like fp6-permission-agent publishes (fp6-permd state + names).
.pragma library
var now = 1793180000;
var state = {
    now: now, broker: true, agent: true, focus_known: true,
    system_apps: ["lomiri-system-settings", "lomiri-dialer-app", "lomiri-messaging-app", "lomiri-terminal-app"],
    confined: ["morph-browser", "de.haeckerfelix.Fragments", "firefox"],
    apps: {
        "morph-browser": { system: false, confined: true, last_seen: now - 60, in_use: false,
            network: { wifi: true, mobile: false, vpn: true, local: true },
            perms: { usb: { decision: "while_using", changed: now - 7200, last_used: now - 300, last_denied: 0, uses: 4, detail: "YubiKey 5 NFC (1050:0407) · HID" },
                     files: { decision: "deny", changed: now - 86400, last_used: 0, last_denied: now - 3600, uses: 0, detail: "" } } },
        "org.example.Serial": { system: false, confined: false, last_seen: now - 30, in_use: true,
            network: { wifi: false, mobile: false, vpn: false, local: false },
            perms: { usb: { decision: "allow", changed: now - 600, last_used: now - 30, last_denied: 0, uses: 12, detail: "Arduino Uno (2341:0043) · serial" },
                     bluetooth: { decision: "ask", changed: 0, last_used: 0, last_denied: now - 40, uses: 0, detail: "" } } },
        "de.haeckerfelix.Fragments": { system: false, confined: true, last_seen: now - 9000, in_use: false,
            network: { wifi: true, mobile: false, vpn: true, local: true }, perms: {} },
        "lomiri-system-settings": { system: true, confined: false, last_seen: now - 10, in_use: false,
            network: { wifi: true, mobile: true, vpn: true, local: true },
            perms: { usb: { decision: "ask", changed: 0, last_used: now - 10, last_denied: 0, uses: 2, detail: "" } } }
    },
    names: {
        "morph-browser": { name: "Morph Browser", icon: "image://theme/stock_website" },
        "org.example.Serial": { name: "Serial Monitor", icon: "image://theme/stock_usb" },
        "de.haeckerfelix.Fragments": { name: "Fragments", icon: "image://theme/save-to" },
        "lomiri-system-settings": { name: "System Settings", icon: "image://theme/settings" }
    },
    installed: [
        { id: "lomiri-calculator-app", name: "Calculator", icon: "image://theme/calculator-app-symbolic" },
        { id: "lomiri-clock-app", name: "Clock", icon: "image://theme/alarm-clock" },
        { id: "de.haeckerfelix.Fragments", name: "Fragments", icon: "image://theme/save-to" },
        { id: "morph-browser", name: "Morph Browser", icon: "image://theme/stock_website" },
        { id: "io.github.rinigus.PureMaps", name: "Pure Maps", icon: "image://theme/location" },
        { id: "lomiri-system-settings", name: "System Settings", icon: "image://theme/settings" }
    ],
    history: [
        { time: now - 30, app: "org.example.Serial", perm: "usb", kind: "used", detail: "Arduino Uno (2341:0043) · serial" },
        { time: now - 40, app: "org.example.Serial", perm: "bluetooth", kind: "denied", detail: "" },
        { time: now - 120, app: "org.example.Serial", perm: "network", kind: "changed", detail: "wifi blocked" },
        { time: now - 300, app: "morph-browser", perm: "usb", kind: "used", detail: "YubiKey 5 NFC (1050:0407) · HID" },
        { time: now - 310, app: "morph-browser", perm: "usb", kind: "prompted", detail: "allowed while using" },
        { time: now - 900, app: "lomiri-camera-app", perm: "camera", kind: "used", detail: "" },
        { time: now - 3600, app: "morph-browser", perm: "files", kind: "denied", detail: "" },
        { time: now - 7300, app: "io.github.rinigus.PureMaps", perm: "location", kind: "used", detail: "" }
    ],
    grants: [ { app: "org.example.Serial", perm: "usb", node: "/dev/ttyACM0", since: now - 30 } ]
};
var prompt = {
    id: 7, app: "org.example.Serial", name: "Serial Monitor", icon: "image://theme/stock_usb", perm: "usb",
    title: "USB devices", question: "Allow Serial Monitor to access a USB device?",
    detail: "Arduino Uno (2341:0043) · serial"
};
// With the media option: camera / microphone / location from fp6-permd
// (the Camera app's presets, an "only this time" microphone grant).
var mediaState = JSON.parse(JSON.stringify(state));
mediaState.media = ["camera", "microphone", "location"];
mediaState.apps["lomiri-camera-app"] = { system: false, confined: true, last_seen: now - 900, in_use: false,
    network: { wifi: true, mobile: true, vpn: true, local: true },
    perms: { camera: { decision: "allow", preset: true, once: false, changed: 0, last_used: now - 900, last_denied: 0, uses: 3, detail: "" },
             microphone: { decision: "allow", preset: true, once: false, changed: 0, last_used: 0, last_denied: 0, uses: 0, detail: "" } } };
mediaState.apps["org.example.Recorder"] = { system: false, confined: false, last_seen: now - 20, in_use: false,
    network: { wifi: true, mobile: true, vpn: true, local: true },
    perms: { microphone: { decision: "ask", preset: false, once: true, changed: 0, last_used: now - 20, last_denied: 0, uses: 1, detail: "" } } };
mediaState.apps["io.github.rinigus.PureMaps"] = { system: false, confined: true, last_seen: now - 7300, in_use: false,
    network: { wifi: true, mobile: true, vpn: true, local: true },
    perms: { location: { decision: "while_using", preset: true, once: false, changed: 0, last_used: now - 7300, last_denied: 0, uses: 5, detail: "" } } };
mediaState.names["lomiri-camera-app"] = { name: "Camera", icon: "image://theme/camera-app" };
mediaState.names["org.example.Recorder"] = { name: "Voice Recorder", icon: "image://theme/audio-input-microphone-symbolic" };
var mediaPrompt = {
    id: 8, app: "org.example.Recorder", name: "Voice Recorder", icon: "image://theme/audio-input-microphone-symbolic", perm: "microphone",
    title: "Microphone", question: "Allow Voice Recorder to use the microphone?", detail: "",
    options: ["allow", "once", "deny"]
};
