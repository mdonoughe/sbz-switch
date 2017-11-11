# sbz-switch

> Utility for switching Sound Blaster outputs on Windows

The Sound Blaster drivers, at least for the Sound Blaster Z, expose the speaker and headphone outputs as a single audio device to Windows, meaning the normal Windows methods of switching the sound output device will not work. Creative provides a graphical utility for this, but does it does not support hotkeys or anything like that, and it does not maintain a separate volume level for headphones vs speakers.

This is a simple utility that does the following:

1. Mute the sound output.

2. Change the Sound Blaster configuration.

3. Optionally adjust the volume.

4. Unmute.

It's designed to be easily triggered by a hotkey or something.

This may have bugs. Use at your own risk, especially if you have configured your headphones/speakers in a way that they could be damaged by maximum volume sound output during the switch.

## Usage

### Set

> Set a small number of parameters

Switch to speakers at 60% volume with effects turned on:

    sbz-switch set -i "Device Control" SelectOutput 1 -b EfxMasterControl "THXEfx Master OnOff" true -v 60

Switch to headphones at 10% volume with effects turned off:

    sbz-switch set -i "Device Control" SelectOutput 0 -b EfxMasterControl "THXEfx Master OnOff" false -v 10

### Dump

> See or save the current parameters

See the current settings:

    sbz-switch dump

Save the current settings to headphones.toml:

    sbz-switch dump -o headphones.toml

### Apply

> Set many parameters at once

Apply the previously saved headphones.toml file:

    sbz-switch apply -f headphones.toml

Omitting the `-f` parameter will cause sbz-switch to read settings from stdin.

Partial dumps are acceptable input for the apply command, in which case the other parameters are left as is. This means it's possible to use a small toml files like this one:

```toml
[creative."Device Control"]
SelectOutput = 0

[creative.EfxMasterControl]
"THXEfx Master OnOff" = false

[endpoint]
volume = 0.1
```

## Known issues

There may be a pop during the switch, or applications outputting audio may get confused. This seems to be a problem on Creative's end and happens for me even when switching using the official software.
