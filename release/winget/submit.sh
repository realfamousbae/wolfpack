#!/bin/sh
# Opens a pull request to microsoft/winget-pkgs with the manifests that manifests.sh wrote:
# submit.sh VERSION DIR [new]. It works through the GitHub API with the account of `gh` (or the
# token in GH_TOKEN): the fork <account>/winget-pkgs gets a branch with the three files. `new` is
# for the first version of the package. Microsoft asks the author of the first pull request to sign
# its contributor license agreement.
set -eu
version=$1
dir=$2
kind=${3:-version}
upstream=microsoft/winget-pkgs
id=realfamousbae.wolfpack
path="manifests/r/realfamousbae/wolfpack/$version"
user=$(gh api user -q .login)
gh repo fork "$upstream" --clone=false >/dev/null 2>&1 || true
# A new fork takes a moment to exist.
tries=0
until gh api "repos/$user/winget-pkgs" >/dev/null 2>&1; do
    tries=$((tries + 1))
    [ "$tries" -lt 30 ] || { echo "the fork $user/winget-pkgs does not appear" >&2; exit 1; }
    sleep 10
done
gh repo sync "$user/winget-pkgs" -b master >/dev/null
base=$(gh api "repos/$user/winget-pkgs/git/ref/heads/master" -q .object.sha)
branch="$id-$version"
gh api "repos/$user/winget-pkgs/git/refs" -f ref="refs/heads/$branch" -f sha="$base" >/dev/null
for file in "$dir/$path"/*.yaml; do
    name=$(basename "$file")
    gh api -X PUT "repos/$user/winget-pkgs/contents/$path/$name" \
        -f message="$id $version: $name" -f branch="$branch" \
        -f content="$(base64 < "$file" | tr -d '\n')" >/dev/null
done
if [ "$kind" = new ]; then
    title="New package: $id version $version"
else
    title="New version: $id version $version"
fi
gh pr create -R "$upstream" -H "$user:$branch" -B master --title "$title" --body \
"The manifests of wolfpack $version, made by release/winget/manifests.sh of https://github.com/realfamousbae/wolfpack: the release archive installed as a portable command \`wolf\`."
