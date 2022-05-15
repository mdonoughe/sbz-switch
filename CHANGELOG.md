# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.1.0] - 2022-05-15
### Added
- Support for AE-5.

## [4.0.0] - 2020-05-14
### Changed
- Now compatible with winapi 0.3.8 and futures 0.3.

## [3.1.1] - 2019-05-25
### Fixed
- Event monitor should no longer deadlock.

## [3.1.0] - 2019-02-17
### Added
- There is now a `watch_with_volume` method on the API which allows API users to observe both changes to SoundBlaster settings and changes to the Windows volume settings at the same time without needing to run two threads.

### Changed
- The output of the `watch` command is now different due to using the `watch_with_volume` API.

## [3.0.0] - 2019-01-14

This release unfortunately renames the `-f` command line parameter to `-i` to allow for a new `-f` to specify the file format.

### Added
- The `watch` command dumps out a stream of events such as parameters changing, even if those changes are made from another program.
- Output can be written in json or yaml format in addition to toml.

### Fixed
- Error codes from ctsndcr are now checked. This may expose ordering problems during certain transitions, such as if you try to switch between headphones and 5.1 surround with bass management enabled, because bass management is not applicable with headphones. Previously, the operation would silently fail.

### Changed
- It is no longer necessary to initialize COM before calling the API.

## [2.0.0] - 2018-08-11
### Added
- It is now possible to specify a device ID, allowing Sound Blaster settings to change even when another device is marked as default. As a result, the API methods now have an additional parameter for providing the device ID.

## [1.1.0] - 2017-11-13
### Added
- Muting can be disabled by passing `-m false`.

## [1.0.0] - 2017-11-11
### Added
- Dump command to show or save current configuration.
- Apply command to restore a saved configuration.

### Changed
- Previous functionality for switching the output device has changed significantly. `sbz-switch --speakers 3003 --volume 60` becomes `sbz-switch set -i "Processing Control" SpeakerConfig 12291 --volume 60` (3003 was a hex value and 12291 is decimal), however it seems `-i "Device Control" SelectOutput 1` is a better way of doing the same thing. See README.md for more information about the new syntax.

## 0.1.0 - 2017-10-30
### Added
- Command to switch speaker configuration and adjust volume.

[Unreleased]: https://github.com/mdonoughe/sbz-switch/compare/v4.1.0...HEAD
[4.1.0]: https://github.com/mdonoughe/sbz-switch/compare/v4.0.0...v4.1.0
[4.0.0]: https://github.com/mdonoughe/sbz-switch/compare/v3.1.1...v4.0.0
[3.1.1]: https://github.com/mdonoughe/sbz-switch/compare/v3.1.0...v3.1.1
[3.1.0]: https://github.com/mdonoughe/sbz-switch/compare/v3.0.0...v3.1.0
[3.0.0]: https://github.com/mdonoughe/sbz-switch/compare/v2.0.0...v3.0.0
[2.0.0]: https://github.com/mdonoughe/sbz-switch/compare/v1.1.0...v2.0.0
[1.1.0]: https://github.com/mdonoughe/sbz-switch/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/mdonoughe/sbz-switch/compare/v0.1.0...v1.0.0
