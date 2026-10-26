#!/bin/sh
FIREFOX_DIR="$1"
DESTDIR="$2"
MODE="$3"

if [ "$MODE" = "flatpak" ]; then
	echo "Installing into the Firefox flatpak's systemconfig extension dir:"
	echo "  $FIREFOX_DIR"
	if command -v flatpak >/dev/null 2>&1 \
		&& ! flatpak info org.mozilla.firefox >/dev/null 2>&1; then
		echo
		echo "WARNING: the org.mozilla.firefox flatpak does not seem to be"
		echo "installed (for this user)."
		echo
	fi
	exit 0
fi

if ! [ -d "$FIREFOX_DIR"/browser ]; then
	if [ -d "$FIREFOX_DIR"-esr/browser ]; then
		echo
		echo "ERROR: Firefox ESR is installed, but FIREFOX_DIR points to"
		echo "       regular Firefox."
		echo
		echo "Run this instead:"
		echo "$ sudo make FIREFOX_DIR=$FIREFOX_DIR-esr install"
		echo
		exit 1
	else
		echo
		echo "WARNING: FIREFOX_DIR does not exist: $FIREFOX_DIR"
		echo "This should be fine during packaging. However if you just ran"
		echo "'make install' manually, consider running 'make uninstall', then"
		echo "adjusting FIREFOX_DIR (in the Makefile or via environment"
		echo "variable) and running 'make install' again."
		echo
	fi
fi

if [ -z "$DESTDIR" ] && [ "$(id -u)" != 0 ]; then
	echo
	echo "ERROR: run make install as root!"
	echo
	echo "$ sudo make FIREFOX_DIR=$FIREFOX_DIR install"
	echo
	exit 1
fi
