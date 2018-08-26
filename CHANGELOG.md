# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Fixed
- Error codes from ctsndcr are now checked. This may expose ordering problems during certain transitions, such as if you try to switch between headphones and 5.1 surround with bass management enabled, because bass management is not applicable with headphones. Previously, the operation would silently fail.

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

[Unreleased]: https://github.com/mdonoughe/sbz-switch/compare/v2.0.0...HEAD
[2.0.0]: https://github.com/mdonoughe/sbz-switch/compare/v1.1.0...v2.0.0
[1.1.0]: https://github.com/mdonoughe/sbz-switch/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/mdonoughe/sbz-switch/compare/v0.1.0...v1.0.0
