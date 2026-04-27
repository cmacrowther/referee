#!/usr/bin/env node
const fs = require("fs");
const path = require("path");

const pkgPath = path.resolve(__dirname, "../package.json");
const tauriConfPath = path.resolve(__dirname, "../desktop/native/tauri.conf.json");
const cargoTomlPath = path.resolve(__dirname, "../desktop/native/Cargo.toml");

const { version } = JSON.parse(fs.readFileSync(pkgPath, "utf8"));

const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, "utf8"));
tauriConf.version = version;
fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 4) + "\n");
console.log(`tauri.conf.json version set to ${version}`);

const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
const updatedCargoToml = cargoToml.replace(/^version = ".*"/m, `version = "${version}"`);
fs.writeFileSync(cargoTomlPath, updatedCargoToml);
console.log(`Cargo.toml version set to ${version}`);
