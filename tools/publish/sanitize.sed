# Applied by export-public.sh to every exported text file (sed -E).
# Personal absolute paths -> neutral wording.
s#/home/[a-z_][a-z0-9_-]*/Desktop/and#<workspace>#g
s#/home/[a-z_][a-z0-9_-]*/Desktop/fp6-nixos#<public-repo>#g
s#/home/[a-z_][a-z0-9_-]*/Desktop/fp6-upstream#<fp6-upstream>#g
