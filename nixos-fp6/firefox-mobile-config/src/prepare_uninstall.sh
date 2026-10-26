#!/bin/sh -e
FIREFOX_DIR="$1"
DESTDIR="$2"
MODE="$3"

echo

if [ "$MODE" != "flatpak" ] && [ -z "$DESTDIR" ] && [ "$(id -u)" != 0 ]; then
	echo "ERROR: run 'make uninstall' as root"
	echo
	echo "For example:"
	echo "$ sudo make FIREFOX_DIR=$FIREFOX_DIR uninstall"
	echo
	exit 1
fi

echo "You are about to delete some files and directories, where you may"
echo "have other firefox customizations than mobile-config-firefox."
echo "Check the Makefile if unsure."
echo
if [ "$MODE" = "flatpak" ]; then
	echo "This removes mobile-config-firefox from the systemconfig extension"
	echo "dir of the Firefox flatpak:"
	echo "  $FIREFOX_DIR"
	echo
else
	echo "If you want to go back to your distribution's version of"
	echo "mobile-config-firefox, make sure to reinstall the package"
	echo "afterwards with the package manager (apk, apt, pacman, ...)."
	echo
fi
echo -n "Do you really want to uninstall mobile-config-firefox? [y/N] "

read answer

case "$answer" in
	[yY])
		exit 0;;
	*)
		exit 1;;
esac
