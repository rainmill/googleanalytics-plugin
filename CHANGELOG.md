# Changelog

Notable changes to `rainmill-googleanalytics`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) as Cargo reads it: before 1.0 a minor
bump is breaking and a patch is additive.

## [Unreleased]

### Added

- First cut. Google Analytics 4 on the report dashboards through the Data API, with a shipped dashboard and a connection test, and a tracker component whose consent is left to the site: none, wait for a grant, or Google Consent Mode v2.
