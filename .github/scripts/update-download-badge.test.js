// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

const assert = require('node:assert/strict')
const test = require('node:test')

const {
  badgePayload,
  distributionDownloadTotal,
  isDistributionAsset,
} = require('./update-download-badge.js')

test('includes installers and actual updater packages', () =>
{
  for (const name of [
    'Ugurugu-macOS-arm64.dmg',
    'Ugurugu-Windows-x64-Setup.exe',
    'Ugurugu-0.7.0-full.nupkg',
    'Ugurugu-macOS-arm64.zip',
    'Ugurugu-0.4.0-macOS-arm64.zip',
  ])
  {
    assert.equal(isDistributionAsset(name), true, name)
  }
})

test('keeps counting assets released before the Ugurugu rename', () =>
{
  for (const name of [
    'WagleWaglePaint-macOS-arm64.dmg',
    'WagleWaglePaint-Windows-x64-Setup.exe',
    'WagleWaglePaint-1.1.0-full.nupkg',
    'WagleWaglePaint-macOS-arm64.zip',
    'WagleWaglePaint-0.2.2-macOS-arm64.zip',
  ])
  {
    assert.equal(isDistributionAsset(name), true, name)
  }
})

test('excludes updater metadata requests', () =>
{
  for (const name of [
    'appcast.xml',
    'releases.win.json',
    'assets.win.json',
    'RELEASES',
    'Ugurugu-macOS-arm64.ko.html',
  ])
  {
    assert.equal(isDistributionAsset(name), false, name)
  }
})

test('sums public distribution assets only', () =>
{
  const releases = [
    {
      draft: false,
      assets: [
        { name: 'appcast.xml', download_count: 118 },
        { name: 'Ugurugu-macOS-arm64.dmg', download_count: 15 },
        { name: 'Ugurugu-0.7.0-full.nupkg', download_count: 15 },
      ],
    },
    {
      draft: true,
      assets: [
        { name: 'Ugurugu-Windows-x64-Setup.exe', download_count: 9 },
      ],
    },
  ]
  assert.equal(distributionDownloadTotal(releases), 30)
  assert.equal(JSON.parse(badgePayload(30)).message, '30')
})
