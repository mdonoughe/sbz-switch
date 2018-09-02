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

```
USAGE:
    sbz-switch.exe [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    apply           applies a saved configuration
    dump            prints out the current configuration
    help            Prints this message or the help of the given subcommand(s)
    list-devices    prints out the names and IDs of available devices
    set             sets specific parameters
    watch           watches for events
```

### List Devices

> Find available audio devices

```
USAGE:
    sbz-switch.exe list-devices

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
```

### Set

> Set a small number of parameters

```
USAGE:
    sbz-switch.exe set [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -b <FEATURE> <PARAMETER> <true|false>        Sets a boolean value
    -d, --device <DEVICE_ID>                     Specify the device to act on (get id from list-devices)
    -f <FEATURE> <PARAMETER> <VALUE>             Sets a floating-point value
    -i <FEATURE> <PARAMETER> <VALUE>             Sets an integer value
    -m <true|false>                              Temporarily mutes while changing parameters [default: true]
    -v, --volume <VOLUME>                        Sets the volume, in percent
```

Switch to speakers at 60% volume with effects turned on:

    sbz-switch set -i "Device Control" SelectOutput 1 -b EfxMasterControl "THXEfx Master OnOff" true -v 60

Switch to headphones at 10% volume with effects turned off:

    sbz-switch set -i "Device Control" SelectOutput 0 -b EfxMasterControl "THXEfx Master OnOff" false -v 10

### Dump

> See or save the current parameters

```
USAGE:
    sbz-switch.exe dump [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -d, --device <DEVICE_ID>    Specify the device to act on (get id from list-devices)
    -o, --output <FILE>         Saves the current settings to a file
```

See the current settings:

    sbz-switch dump

Save the current settings to headphones.toml:

    sbz-switch dump -o headphones.toml

### Apply

> Set many parameters at once

```
USAGE:
    sbz-switch.exe apply [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -d, --device <DEVICE_ID>    Specify the device to act on (get id from list-devices)
    -f <FILE>                   Reads the settings from a file instead of stdin
    -m <true|false>             Temporarily mutes while changing parameters [default: true]
```

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

### Watch

> Watch for events such as parameter changes

```
USAGE:
    sbz-switch.exe watch [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -d, --device <DEVICE_ID>    Specify the device to act on (get id from list-devices)
```

## Known issues

There may be a pop during the switch, or applications outputting audio may get confused. This seems to be a problem on Creative's end and happens for me even when switching using the official software.

Order matters when setting parameters. Not only does this program not order the parameters correctly itself, but it may even reorder parameters read from a file such that they apply in the wrong order.
