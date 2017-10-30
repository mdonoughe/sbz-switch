# sbz-switch

> Utility for switching Sound Blaster outputs on Windows.

The Sound Blaster drivers, at least for the Sound Blaster Z, expose the speaker and headphone outputs as a single audio device to Windows, meaning the normal Windows methods of switching the sound output device will not work. Creative provides a graphical utility for this, but does it does not support hotkeys or anything like that, and it does not maintain a separate volume level for headphones vs speakers.

This is a simple utility that does the following:

1. Mute the sound output.

2. Change the speaker configuration (e.g. to headphones).

3. Optionally adjust the volume.

4. Unmute.

It's designed to be easily triggered by a hotkey or something and it runs in under a second.

This may have bugs. Use at your own risk, especially if you have configured your headphones/speakers in a way that they could be damaged by maximum volume sound output during the switch.

## Usage

This is what works for me. I haven't played with other speaker configurations.

Switch to speakers:

    sbz-switch --speakers 3003 --volume 60

Switch to headphones:

    sbz-switch --speakers 80000000 --volume 10

## Known issues

There may be a pop during the switch, or applications outputting audio may get confused. This seems to be a problem on Creative's end and happens for me even when switching using the official software.

This isn't a full profiles solution. If you have different settings such as sound enhancement for different outputs this will not apply them.
