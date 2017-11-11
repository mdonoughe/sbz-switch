# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2017-11-11
### Added
- Dump command to show or save current configuration.
- Apply command to restore a saved configuration.

### Changed
- Previous functionality for switching the output device has changed significantly. `sbz-switch --speakers 3003 --volume 60` becomes `sbz-switch set -i "Processing Control" SpeakerConfig 12291 --volume 60` (3003 was a hex value and 12291 is decimal), however it seems `-i "Device Control" SelectOutput 1` is a better way of doing the same thing. See README.md for more information about the new syntax.

## [0.1.0] - 2017-10-30
### Added
- Command to switch speaker configuration and adjust volume.
