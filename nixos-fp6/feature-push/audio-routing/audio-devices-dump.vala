/*
 * Copyright 2026 Paul Schuldesz
 * GPL-3.0-only, as the rest of ayatana-indicator-sound.
 *
 * Read-only diagnostic: prints the output/input choices the sound menu
 * would offer (same AudioDeviceSelector code) and exits. Selects nothing.
 */
int main (string[] args)
{
    var loop = new MainLoop ();
    var pa_loop = new PulseAudio.GLibMainLoop ();
    var selector = new AudioDeviceSelector (pa_loop);
    selector.read_only = true;
    int status = 1;

    selector.changed.connect (() => {
        print ("in_call: %s\n", selector.in_call ? "true" : "false");
        print ("outputs: %s\n", selector.to_json (AudioDeviceSelector.Kind.OUTPUT));
        print ("inputs: %s\n", selector.to_json (AudioDeviceSelector.Kind.INPUT));
        print ("apps: %s\n", selector.apps_json ());
        print ("switch_to_new_devices: %s\n", selector.auto_switch ? "true" : "false");
        status = 0;
        loop.quit ();
    });
    Timeout.add_seconds (5, () => {
        printerr ("no answer from the sound server\n");
        loop.quit ();
        return Source.REMOVE;
    });
    loop.run ();
    return status;
}
