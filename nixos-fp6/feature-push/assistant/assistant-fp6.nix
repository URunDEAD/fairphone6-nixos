# Assistant (feature push 2026-10-24): "an app similar to what Volla has,
# that Springboard": type (or say) what you want; a local or remote LLM
# agent does it with the phone's apps, asking for each resource it needs.
# Design, permission model, previews, phone checklist:
# results/feature-push-20261024/assistant/README.md
#
#  * fp6-assistantd (Rust, user service, D-Bus com.fairphone6.Assistant1):
#    chats, the agent loop, model backends (on-device com.fairphone6.LocalLlm1
#    with a manual-URL fallback; OpenAI-compatible URL; Anthropic Messages
#    API; Ollama on the LAN), tools against the phone's services
#    (history-service / telephony-service over oFono, qtcontacts-sqlite,
#    IMAP/SMTP, files in granted folders, screenshots, camera via Content
#    Hub, OCR, calendar/alarms via mkcal, indicator switches, GeoClue,
#    notification history, web, share sheet, KDE Connect, url-dispatcher)
#    and JSON tool manifests (/etc/fp6-assistant/tools.d).
#  * Permissions: per resource (Allow once / for this chat / always / deny,
#    folders scoped), per-action confirmation for side effects (relaxable
#    per tool, but always back after untrusted content), rate limits, an
#    activity log per chat, Stop. The app must hold the broker's system
#    permission (fp6-permd, org.fairphone.Permissions1) before it asks.
#  * fp6-assistant: the Lomiri UITK 1.3 app (Springboard, chats, settings).
#  * Lomiri (overlay lib.mkOrder 4300, after every other module's edits):
#    an "Ask the assistant: …" row in the app drawer's search, and an
#    opt-in "Assistant" quick-settings tile. Any gesture (shake, double
#    press, back tap) can open it via Settings > Gesture Shortcuts > App.
{ config, lib, pkgs, options, ... }:
let
  cfg = config.hardware.fairphone6.assistant;
  pkg = pkgs.callPackage ./package.nix { };
  user = config.services.fp6LomiriSensors.user or "user";
  quickSettingsOn = config.hardware.fairphone6.quickSettings.enable or false;
  tileAnchor = "    // ---- page tiles: the indicator pages that have no switch tile of their own";
  drawerAnchor = "            DrawerGridView {\n                id: appList\n";
in {
  options.hardware.fairphone6.assistant = {
    enable = lib.mkEnableOption ''
      the Assistant: a Springboard-style app where a local or remote LLM
      agent uses the phone's apps through permission-gated tools'';
    drawerSearch = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "\"Ask the assistant: …\" row in the Lomiri app drawer's search (Lomiri overlay, lib.mkOrder 4300).";
    };
    quickSettingsTile = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Offer an \"Assistant\" tile in the quick-settings editor (needs hardware.fairphone6.quickSettings).";
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = pkg;
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "The assistant package.";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [ (lib.optionalAttrs (options.services ? fp6Cellular) {
    # GeoClue: the assistant's location tool (still behind its own
    # "Location" permission and the Location switch).
    services.fp6Cellular.gnss.allowedApplications = [ "com.fairphone6.Assistant" ];
  }) {
    environment.systemPackages = [ cfg.package ];
    services.dbus.packages = [ cfg.package ];
    xdg.mime.defaultApplications."x-scheme-handler/assistant" = "com.fairphone6.Assistant.desktop";

    systemd.user.services.fp6-assistant = {
      description = "FP6 Assistant (agent, model backends, permission-gated phone tools)";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = user;
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Assistant1";
        ExecStart = "${cfg.package}/bin/fp6-assistantd";
        Restart = "on-failure";
        RestartSec = 3;
        UMask = "0077";
        NoNewPrivileges = true;
        Nice = 5;
        # Chats, settings, keys and caches only; no RAM-hungry model in here
        # (the on-device runtime is its own service).
        MemoryMax = "256M";
      };
    };

    nixpkgs.overlays = lib.mkIf (cfg.drawerSearch || (cfg.quickSettingsTile && quickSettingsOn)) (lib.mkOrder 4300 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          postPatch = (old.postPatch or "") + lib.optionalString cfg.drawerSearch ''
            # FP6 Assistant: "Ask the assistant" in the drawer search (after every other module's Lomiri edits)
            drawer=qml/Launcher/Drawer.qml
            substituteInPlace $drawer \
              --replace-fail ${lib.escapeShellArg drawerAnchor} "$(cat ${./lomiri/DrawerAskRow.qml.in})
            ${drawerAnchor}" \
              --replace-fail '                    top: searchFieldContainer.bottom' '                    top: fp6AskAssistant.bottom' \
              --replace-fail '                            root.applicationSelected(appList.getFirstAppId());' \
                             '                            if (appList.count > 0) root.applicationSelected(appList.getFirstAppId()); else fp6AskAssistant.ask();'
            grep -q 'id: fp6AskAssistant' $drawer
            grep -q 'top: fp6AskAssistant.bottom' $drawer
          '' + lib.optionalString (cfg.quickSettingsTile && quickSettingsOn) ''
            qs=qml/Panel/QuickSettings/QsBackend.qml
            test -e $qs
            substituteInPlace $qs --replace-fail '${tileAnchor}' \
              "$(cat ${./lomiri/AssistantTile.qml.in})
            ${tileAnchor}"
            sed -i '/readonly property var catalogue: \[/,/\]/ s/\]$/, fp6AssistantTile]/' $qs
            grep -q 'fp6AssistantTile\]' $qs
            grep -q 'tileId: "assistant"' $qs
          '';
          passthru = (old.passthru or { }) // { fp6Assistant = true; };
        });
      });
    }) ]);

    system.build.fp6-assistant = {
      package = cfg.package;
      lomiri = pkgs.lomiri.lomiri;
    };
  } ]);
}
