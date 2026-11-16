# Apache setup for positorium.com

These files target Apache 2.4 on Alpine Linux / OpenRC and the existing
`/etc/apache2/virtual.d/` configuration layout. The canonical address is **https://positorium.com/**;
`www.positorium.com` redirects there. The certificate covers both names.

Run the commands below **on the server as root**. They have not been applied to
the server. Both domain names must resolve to this server, including any AAAA
records, and ports 80 and 443 must be reachable. Apache must already include
`/etc/apache2/virtual.d/*.conf`; use the same inclusion mechanism as the existing
uptochange.com files. The working HTTPS site already establishes the port-443
listener, so do not add a duplicate `Listen 443` directive.

Alpine uses `rc-service apache2 reload` to reload Apache. Its modules are loaded
through `LoadModule` directives in Apache's configuration, rather than Debian's
`a2enmod`. Check `apache2ctl -M` for `ssl_module`, `rewrite_module`, `mime_module`,
`headers_module`, and `authz_core_module`, which the supplied virtual hosts need.

## 1. Upload the website and configuration files

Upload `positorium-website.zip` and `positorium-apache.zip` to `/root/`, then:

```sh
install -d -m 0755 /var/www/www.positorium.com
unzip /root/positorium-website.zip -d /var/www/www.positorium.com
unzip /root/positorium-apache.zip -d /root/positorium-apache
chmod -R a+rX /var/www/www.positorium.com
```

`/var/www/www.positorium.com/index.html` should now exist. Apache configuration
files belong outside the web root. The website's `.htaccess` is unnecessary
with these virtual hosts: `AllowOverride None` is intentional, and the MIME
settings are supplied in the virtual host itself.

## 2. Replace the GitHub redirect with temporary HTTP hosting

Back up the existing Positorium HTTP file outside the included directory. The
commands use `http.www.positorium.com.conf`; if the current filename differs,
replace that file instead so two matching port-80 hosts are not loaded.

```sh
cp -an /etc/apache2/virtual.d/http.www.positorium.com.conf /root/http.www.positorium.com.conf.before-website
cp /root/positorium-apache/http.www.positorium.com.bootstrap /etc/apache2/virtual.d/http.www.positorium.com.conf
apache2ctl configtest && rc-service apache2 reload
```

At this point the website should load over HTTP, with no GitHub redirect. Do not
install the HTTPS configuration yet: its certificate files do not exist until
the next step succeeds. Keep the bootstrap source outside `virtual.d`, even
though its filename does not end in `.conf`.

## 3. Issue a certificate for both names

```sh
certbot certonly --webroot \
  --webroot-path /var/www/www.positorium.com \
  --cert-name positorium.com \
  -d positorium.com -d www.positorium.com
```

This uses the already-running Apache server to answer HTTP-01 challenges, without
letting Certbot rewrite the custom virtual-host configuration. `--cert-name`
sets the certificate lineage used by the HTTPS file. Reuse this server's existing
deployment hook, which already reloads Apache and the mail services. A second
certificate-specific Apache reload hook is unnecessary. Keep the existing
Certbot renewal cron/periodic job; a deploy hook alone does not schedule renewals.

Confirm Certbot reports these paths:

```text
/etc/letsencrypt/live/positorium.com/fullchain.pem
/etc/letsencrypt/live/positorium.com/privkey.pem
```

If using a pre-existing certificate instead, it must cover **both** names, and
the two certificate paths in the HTTPS configuration must match it.

## 4. Enable HTTPS and the permanent redirects

```sh
cp /root/positorium-apache/https.www.positorium.com.conf /etc/apache2/virtual.d/https.www.positorium.com.conf
cp /root/positorium-apache/http.www.positorium.com.conf /etc/apache2/virtual.d/http.www.positorium.com.conf
apache2ctl configtest && rc-service apache2 reload
```

The existing `/etc/letsencrypt/options-ssl-apache.conf` is reused, as it is for
uptochange.com. The final HTTP host leaves `/.well-known/acme-challenge/` on HTTP
for renewals, while other HTTP requests redirect to HTTPS. The HTTPS host sends
`www` to the canonical apex domain, retaining the path and query string.

The old `Redirect / https://github.com/...` must be removed completely from the
active Positorium configuration. `Require all granted` replaces the Apache 2.2
`Order`/`Allow` syntax. The obsolete MSIE workaround and duplicate `ServerAdmin`
are omitted. The full document root is granted access, not a `desolate` subfolder.

## 5. Verify

```sh
apache2ctl -S
curl -I http://positorium.com/studio.html
curl -I http://www.positorium.com/studio.html
curl -I https://www.positorium.com/studio.html
curl -I https://positorium.com/
curl -I https://positorium.com/pkg/positorium_bg.wasm
certbot renew --cert-name positorium.com --dry-run --run-deploy-hooks
```

The first three requests should return `301` with
`Location: https://positorium.com/studio.html`. The last two should return `200`;
the WASM response should have `Content-Type: application/wasm`. Finally, run a
query in the homepage playground and open the full Query Studio. The renewal
dry run tests validation, and `--run-deploy-hooks` also exercises the existing
deployment hook using the active certificate. Check for hook errors as well as
successful simulated renewal.

## Removing the redundant systemd hook from the initial instructions

The earlier instructions assumed systemd. On this Alpine host, that extra hook
fails with exit 127 because `systemctl` does not exist. The existing deployment
hook already reloads Apache successfully, so remove only the added hook from
the Positorium certificate's renewal configuration.

Back up the file outside the renewal directory, then locate the hook:

```sh
cp -an /etc/letsencrypt/renewal/positorium.com.conf /root/positorium.com-renewal.conf.before-hook-fix
grep -nE '^[[:space:]]*(renew_hook|deploy_hook)[[:space:]]*=' /etc/letsencrypt/renewal/positorium.com.conf
```

Edit `/etc/letsencrypt/renewal/positorium.com.conf` and remove only the line whose
command is `apache2ctl configtest && systemctl reload apache2`. Certbot commonly
stores the CLI deploy hook under the name `renew_hook`:

```ini
renew_hook = apache2ctl configtest && systemctl reload apache2
```

Keep the existing deployment script and other certificate settings. Then rerun:

```sh
certbot renew --cert-name positorium.com --dry-run --run-deploy-hooks
```

If a separate certificate-specific hook is wanted instead, its Alpine command
is `apache2ctl configtest && rc-service apache2 reload`. With the existing hook
this would reload Apache twice.

References: [Apache redirects and ACME exceptions](https://httpd.apache.org/docs/2.4/rewrite/remapping.html),
[Apache 2.4 access control](https://httpd.apache.org/docs/2.4/upgrading.html#access),
[Certbot webroot and renewal](https://eff-certbot.readthedocs.io/en/stable/using.html).
