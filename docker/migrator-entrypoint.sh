#!/bin/sh
set -e

if [ -n "$DATABASE_URL_FILE" ]; then
	DATABASE_URL=$(cat "$DATABASE_URL_FILE")
	export DATABASE_URL
fi

exec /bin/migration "$@"
