# Mono feedback for your Mono Gateway

Mono asks you to install a small add-on for about one month. It helps us
learn how warm the Mono Gateway runs in real use.

## What it sends

Every 15 minutes, the add-on sends one reading to Mono over an encrypted
(HTTPS) connection:

- the temperatures of the processor, memory, network chips and SFP+ modules;
- the fan speed;
- the power the router takes from its USB-C supply;
- the router's MAC address (`eth0`), so that we can tell readings from
  different routers apart;
- the time of the reading.

It sends nothing else: no traffic, no addresses of your devices, no settings
and no passwords. Mono does not store the IP address that a reading comes from.

We keep the readings for <RETENTION>, and then we delete them. For questions,
or to have your readings deleted earlier, write to <CONTACT>.

## Before you start

The router must run Verso. Connect to it over SSH: `ssh root@192.168.1.1`.

If Verso is not installed yet, install it with the full feed address. The
`$(apk --print-arch)` form in the Verso README gives `aarch64` on the Mono
Gateway, where the feed is at `aarch64_generic`:

```sh
wget -O /etc/apk/keys/verso.pem https://sysupgrade.mono.si/verso/verso.pem
echo "https://sysupgrade.mono.si/verso/aarch64_generic/packages.adb" > /etc/apk/repositories.d/verso.list
apk update && apk add verso
```

## Install

1. Trust the add-on's signing key:

   ```sh
   wget -O /etc/apk/keys/mono-diag.pem <KEY_URL>
   ```

2. Download and install the add-on:

   ```sh
   wget -O /tmp/verso-plugin-diag.apk <APK_URL>
   apk update && apk add /tmp/verso-plugin-diag.apk
   ```

The add-on starts at once. In Verso, open **System › Mono feedback** to see
the router's device ID and when the last reading was sent.

## Remove

Run this when the collection ends, or at any time before that:

```sh
apk del verso-plugin-diag
rm /etc/apk/keys/mono-diag.pem
```

`apk del` stops the add-on and removes it. After that, the router sends no
more readings. `apk del` keeps `curl` installed if apk brought it with the
add-on; remove it with `apk del curl` if nothing else on the router uses it.
