#!/bin/sh
# Builds into ./artifacts (also the referenced library's output, so the sibling checkout stays untouched) and runs.
here=$(cd "$(dirname "$0")" && pwd)
exec dotnet run -c Release --project "$here/Compare.csproj" -p:UseArtifactsOutput=true -p:ArtifactsPath="$here/artifacts" -- "$@"
