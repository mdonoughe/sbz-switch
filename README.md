# sbz-switch

> Utility for switching Sound Blaster outputs on Windows

[![Crates.io](https://img.shields.io/crates/v/sbz-switch.svg)](https://crates.io/crates/sbz-switch) ![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg) [![Build status](https://ci.appveyor.com/api/projects/status/554198r095ibw7ma?svg=true)](https://ci.appveyor.com/project/mdonoughe/sbz-switch) [![Docs.rs](https://docs.rs/sbz-switch/badge.svg)](https://docs.rs/sbz-switch)

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
    apply           Applies a saved configuration
    dump            Prints out the current configuration
    help            Prints this message or the help of the given subcommand(s)
    list-devices    Prints out the names and IDs of available devices
    set             Sets specific parameters
    watch           Watches for events
```

### List Devices

> Find devices to control

```
USAGE:
    sbz-switch.exe list-devices

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -f <FORMAT>        Select the output format [default: toml]  [possible values: toml, json, yaml]
```

If the Sound Blaster is not the default audio output, execute `list-devices` to get the device ID.

```toml
[[]]
id = '{0.0.0.00000000}.{cba07706-3492-4789-bb31-0717e228bd14}'
interface = 'Sound Blaster Z'
description = 'Speakers'

[[]]
id = '{0.0.0.00000000}.{baeaa072-e026-44e1-942e-c466170d9d6f}'
interface = 'Steam Streaming Microphone'
description = 'Speakers'

[[]]
id = '{0.0.0.00000000}.{57e7b4bc-c860-4987-aed3-3ee8dd3617b9}'
interface = 'Steam Streaming Speakers'
description = 'Speakers'

[[]]
id = '{0.0.0.00000000}.{128e193d-e35f-40dd-b414-16105e5ec32d}'
interface = '2- USB Audio Device'
description = 'Speakers'
```

Pass the device ID to other commands: `dump -d "{0.0.0.00000000}.{cba07706-3492-4789-bb31-0717e228bd14}"`.

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
    -f <FORMAT>        Select the output format [default: toml]  [possible values: toml, json, yaml]
    -o, --output <FILE>         Saves the current settings to a file
```

See the current settings:

    sbz-switch dump

Save the current settings to headphones.toml:

    sbz-switch dump -o headphones.toml

Note: saving parameters this way will include many parameters, some of which may not actually be settable when used with the `apply` command. It is recommended to remove unnecessary settings to speed up the transition and avoid errors.

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

Partial dumps are acceptable (and recommended) input for the apply command, in which case the other parameters are left as is. This means it's possible to use a small toml files like these:

#### headphones.toml
```toml
[creative."Device Control"]
SelectOutput = 0

[creative.EfxMasterControl]
"THXEfx Master OnOff" = false

[endpoint]
volume = 0.1
```

#### speakers.toml
```toml
[creative."Device Control"]
SelectOutput = 1

[creative.EfxMasterControl]
"THXEfx Master OnOff" = true

[endpoint]
volume = 0.6
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

Some parameters are only valid if another parameter has been set or when using certain hardware, e.g. 7.1 surround sound speaker configuration. Unfortunately, these parameters will be included in a full parameter dump and may lead to errors when reapplying the settings later. It should be generally safe to ignore such errors, but they can be avoided by removing the offending settings from the dump file.

Order matters when setting parameters. This program make no attempt to order the parameters correctly itself. Additionally, toml files are read using [toml](https://crates.io/crates/toml) 0.4 which does not maintain the order of parameters.
