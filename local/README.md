# local/ (untracked)

Machine-specific settings. Everything in this directory except this README is
git-ignored. Create the files you need:

`local/phone.env`, read by `tools/phone-ssh`:

```sh
FP6_SSH_PASSWORD='the phone login password'
# FP6_SSH_ADDRESS=<phone Wi-Fi address>   # optional fallback when USB is not connected
# FP6_SSH_KEYS=1                           # use SSH keys instead of a password
```

`local/builder.env`, read by `tools/nix-remote`:

```sh
FP6_BUILDER_STORE='ssh-ng://builder@your-arm-builder?ssh-key=/home/you/.ssh/fp6-builder'
```

Use `chmod 600 local/*.env`.
