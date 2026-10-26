// Copyright 2022 Oliver Smith, Martijn Braam
// SPDX-License-Identifier: MPL-2.0

// Set up autoconfig (we use it to copy/update userChrome.css into profile dir)
pref('general.config.filename', "mobile-config-autoconfig.js");
pref('general.config.obscure_value', 0);
pref('general.config.sandbox_enabled', false);

// Enable touch density
pref('browser.uidensity', 2);

// Enable Pipewire cameras
pref('media.webrtc.camera.allow-pipewire', true);
