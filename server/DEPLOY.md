# Hosting the Flexspeak collection server

This guide is for the person who runs the server. You don't need to know Rust or how the server is built. You need to be comfortable typing commands on a Linux machine over SSH.

Wherever you see `collection.example.com`, use your own server name. Wherever you see `example.com`, use your own domain.

## 1. What this is, and what you need

Flexspeak is a TeamSpeak client. Its "Monster collection" page keeps people's collections, their accounts and their synced settings on a small server. This guide sets up that server.

The setup has three parts, each running in its own Docker container:

- **monster**: the server itself. It keeps everything in one database file.
- **Caddy**: a web server in front of it. It is the only part reachable from the internet, and it gets and renews the HTTPS certificate by itself.
- **Watchtower**: checks every 5 minutes whether a new version of the server was published, and if so, installs it and restarts the server.

You need:

1. **A Linux machine with Docker** that is on all the time and has a public IP address. A small virtual server is enough: the server uses very little memory and disk.
2. **A domain name** you control, so you can add DNS records to it (for example `example.com`). The server will live at a name under it, such as `collection.example.com`.
3. **An account with Resend** (resend.com), which sends the emails with sign-in codes. Section 3 explains how to set it up.

## 2. Point a name at the machine (DNS)

1. Find the public IP address of your machine. Your hosting provider shows it, or run this on the machine:

   ```sh
   curl -4 https://ifconfig.me
   ```

2. In the DNS settings of your domain (at the company where you registered it, or wherever its DNS is managed), add a record:

   | Type | Name | Value |
   | --- | --- | --- |
   | `A` | `collection` | the IP address from step 1 |

   The name `collection` makes `collection.example.com`. You can choose another name, but choose carefully: **it must never change later**, because every passkey people create is tied to it (see [Troubleshooting](#passkeys-stop-working-after-the-address-changed)).

3. Wait until the name answers with your IP address. This can take from a few minutes to a few hours. Check it with:

   ```sh
   nslookup collection.example.com
   ```

## 3. Set up email sending with Resend

People sign in with a six-digit code the server emails them. Resend sends these emails. The menus below were right at the time of writing; if something looks different, check Resend's current documentation.

1. Create an account at https://resend.com.
2. In the dashboard, open **Domains** and choose **Add Domain**. Enter the domain the emails should come from, such as `example.com`.
3. Resend shows a list of DNS records (usually a few `TXT` records and one `MX` record). Add each of them, exactly as shown, in the DNS settings of that domain, the same place as in section 2. These records prove to other mail servers that Resend may send for your domain.
4. Back in Resend, start the verification on the domain's page. Wait until the domain shows as **Verified**. This can take from minutes to a few hours.
5. On the same domain's page, turn **click tracking** off (and open tracking too, if it is on). Click tracking rewrites links in emails, which the sign-in emails don't need and which makes them look less trustworthy.
6. Open **API Keys** and choose **Create API Key**. Give it a name such as `flexspeak-collection`. Set its permission to **Sending access** only, and if offered, limit it to the domain from step 2.
7. Copy the key. It starts with `re_` and is shown only once. You will put it in the `.env` file in section 4.
8. Choose the sender address for the emails, such as `collection@example.com`. It must be on the domain you verified. It doesn't need a real mailbox.

Resend's free plan allowed about 100 emails a day at the time of writing; check their pricing page. Each sign-in with a code sends one email; signing in with a passkey sends none.

## 4. First start on the machine

All commands below are run on the machine, over SSH. They use `sudo` because Docker needs administrator rights.

1. Install Docker Engine with its Compose plugin. Follow the instructions for your Linux distribution at https://docs.docker.com/engine/install/. Then check that both work:

   ```sh
   sudo docker version
   sudo docker compose version
   ```

2. Make sure the machine accepts connections on ports 80 and 443. If your hosting provider has a firewall in its web dashboard, allow incoming TCP on ports 80 and 443 there (and UDP on 443, which makes HTTPS a little faster but isn't required). Port 80 is needed for getting the certificate.

3. Make a folder for the server and go into it:

   ```sh
   sudo mkdir -p /opt/flexspeak-monster
   cd /opt/flexspeak-monster
   ```

4. Download the four files of the setup:

   ```sh
   base=https://raw.githubusercontent.com/zirkumflex-group/flexspeak/main/server/deploy
   sudo curl -fsSL -o compose.yaml "$base/compose.yaml"
   sudo curl -fsSL -o Caddyfile "$base/Caddyfile"
   sudo curl -fsSL -o .env.example "$base/.env.example"
   sudo curl -fsSL -o backup.sh "$base/backup.sh"
   sudo chmod +x backup.sh
   ```

5. Make your own settings file from the example, and make it readable only by administrators, since it holds your Resend key:

   ```sh
   sudo cp .env.example .env
   sudo chmod 600 .env
   ```

6. Open it in a text editor:

   ```sh
   sudo nano .env
   ```

   Fill in every value. Each one has a comment above it saying what it is:

   - `DOMAIN`: the name from section 2, such as `collection.example.com`, without `https://`.
   - `ACME_EMAIL`: your own email address. The certificate authority writes to it only if something is wrong with the certificate.
   - `RESEND_API_KEY`: the key from section 3, step 7.
   - `MAIL_FROM`: a name and the sender address from section 3, step 8, in double quotes, such as `"Flexspeak <collection@example.com>"`.

   In nano, save with Ctrl+O and Enter, and leave with Ctrl+X.

7. Start everything:

   ```sh
   sudo docker compose up -d
   ```

   The first time, this downloads the images, which takes a minute or two. Caddy starts only once the server reports itself healthy.

8. Check that all three containers are running:

   ```sh
   sudo docker compose ps
   ```

   `monster` should say `healthy` after about half a minute.

9. Check the server from the internet. Run this on any computer:

   ```sh
   curl -i https://collection.example.com/health
   ```

   The first line of the answer should be `HTTP/2 200`. Getting the certificate can take a minute after the first start; if this fails at first, wait and try again. If it keeps failing, see [Troubleshooting](#the-https-certificate-isnt-issued).

10. Set up nightly backups now, as described in [section 8](#8-backups).

If step 7 fails right away with "pull access denied" or "unauthorized", the server's image isn't public yet. See section 5.

## 5. Access to the server's image

GitHub publishes the server as a container image at `ghcr.io/zirkumflex-group/flexspeak-monster`. Your machine downloads it from there. Whether it can do so without a password depends on the package's visibility, which a GitHub administrator of the repository sets once. The steps below match GitHub at the time of writing; check GitHub's current documentation if they differ.

**Option A, recommended: make the package public.** An administrator of the `zirkumflex-group` organisation does this once, after the first image was published:

1. On GitHub, open the organisation, then **Packages**, then `flexspeak-monster`.
2. Open **Package settings**.
3. Under **Danger Zone**, choose **Change visibility** and set it to **Public**.

If public packages aren't offered, the organisation's settings may forbid them (organisation **Settings**, then **Packages**). A public package only lets people download the server, which is open source anyway; it doesn't let anyone change it.

**Option B: keep it private and give the machine a read-only token.**

1. A GitHub user with access to the package creates a personal access token (classic) with only the `read:packages` permission: GitHub, **Settings**, **Developer settings**, **Personal access tokens**, **Tokens (classic)**.
2. On the machine, sign Docker in with it. Replace `GITHUB_USER` with that user's GitHub name; paste the token when asked for a password:

   ```sh
   sudo docker login ghcr.io -u GITHUB_USER
   ```

3. Watchtower needs the same sign-in to download updates. In `compose.yaml`, under `watchtower:` and its `volumes:`, add this line below the existing one (check Watchtower's documentation if updates still fail to download):

   ```yaml
         - /root/.docker/config.json:/config.json:ro
   ```

4. Apply it:

   ```sh
   cd /opt/flexspeak-monster
   sudo docker compose up -d
   ```

## 6. Tell people the address

In Flexspeak, each person enters the server's address under **Settings › WIP › Collection Server**:

```
https://collection.example.com
```

Use exactly this form: with `https://`, without a slash or anything else at the end.

## 7. How updates arrive

You don't need to do anything for updates:

1. When a change to the server is merged into the `main` branch on GitHub, a GitHub workflow tests it, builds a new image and publishes it with two names: `latest` and the commit's ID (a long string of letters and digits, called the commit SHA).
2. Watchtower, on your machine, checks every 5 minutes whether `latest` changed. When it did, it downloads the new image, restarts the server with it, and deletes the old image.

A change usually reaches your machine within about half an hour of being merged: the workflow needs some minutes to test and build, then Watchtower notices it at its next check. During the restart the server is unavailable for a few seconds.

To see what Watchtower did:

```sh
cd /opt/flexspeak-monster
sudo docker compose logs --tail 50 watchtower
```

### Updating by hand

To get the newest version immediately instead of waiting:

```sh
cd /opt/flexspeak-monster
sudo docker compose pull monster
sudo docker compose up -d monster
```

To update Caddy and Watchtower themselves (they don't update automatically; doing this every few months is enough):

```sh
cd /opt/flexspeak-monster
sudo docker compose pull
sudo docker compose up -d
```

### Going back to an earlier version

If a new version misbehaves, you can run the build of any earlier commit.

1. Find the commit's SHA. On GitHub, the package page (organisation, **Packages**, `flexspeak-monster`) lists the published versions with their tags; or open the repository's commit list and copy the full SHA of the commit you want.
2. Take a backup first (see section 8):

   ```sh
   sudo /opt/flexspeak-monster/backup.sh
   ```

3. Open `.env` and add a line at the end, with the SHA in place of `<commit sha>`:

   ```sh
   sudo nano /opt/flexspeak-monster/.env
   ```

   ```
   MONSTER_IMAGE=ghcr.io/zirkumflex-group/flexspeak-monster:<commit sha>
   ```

4. Apply it:

   ```sh
   cd /opt/flexspeak-monster
   sudo docker compose up -d monster
   ```

The server now stays on that version: Watchtower watches that exact tag, which never changes. To follow new versions again, delete the `MONSTER_IMAGE` line from `.env` and run `sudo docker compose up -d monster` again.

A newer version may have changed the database in a way an older one doesn't understand. If the older version doesn't start, restore the backup from step 2 (section 8).

## 8. Backups

All accounts, collections and settings are in one database file inside a Docker volume (a storage area Docker manages) named `flexspeak-monster_monster-data`. If the machine's disk is lost, so is everything, unless you have backups.

`backup.sh` asks the running server for a consistent copy of the database (copying the file directly while the server writes to it could give a broken copy), saves it as a dated file such as `backups/monster-2026-11-12-031500.sqlite`, and deletes copies older than 14 days.

### Taking a backup by hand

```sh
sudo /opt/flexspeak-monster/backup.sh
```

It prints where it saved the copy. To keep copies somewhere else, give a folder: `sudo /opt/flexspeak-monster/backup.sh /path/to/folder`. To keep them longer than 14 days, set `KEEP_DAYS`: `sudo KEEP_DAYS=30 /opt/flexspeak-monster/backup.sh`.

### Every night, automatically

1. Open the administrator's scheduled tasks (cron):

   ```sh
   sudo crontab -e
   ```

   If it asks which editor to use, choose `nano`.

2. Add this line at the end. It runs the backup every night at 03:15, machine time, and writes what happened to a log file:

   ```
   15 3 * * * /opt/flexspeak-monster/backup.sh >> /opt/flexspeak-monster/backup.log 2>&1
   ```

3. Save and leave (in nano: Ctrl+O, Enter, Ctrl+X).

4. The next day, check that it worked:

   ```sh
   sudo tail /opt/flexspeak-monster/backup.log
   sudo ls -l /opt/flexspeak-monster/backups
   ```

Backups on the same machine don't help if the machine is lost. Copy the `backups` folder to another place regularly, for example with your hosting provider's snapshot feature or by downloading it with `scp`. The copies hold every account's email address: keep them private.

### Restoring a backup

This replaces everything on the server with the backup. Anything that happened after the backup was taken is lost.

1. Go to the folder and see which copies you have:

   ```sh
   cd /opt/flexspeak-monster
   sudo ls backups
   ```

2. Stop the server:

   ```sh
   sudo docker compose stop monster
   ```

3. Put the copy in place. Replace `monster-2026-11-12-031500.sqlite` with the file you chose in step 1. This removes the database's working files and gives the copy to the server's user:

   ```sh
   sudo docker compose run --rm --no-deps --user root -v "$PWD/backups:/backups:ro" monster \
     sh -c "rm -f /data/monster.sqlite-wal /data/monster.sqlite-shm && cp /backups/monster-2026-11-12-031500.sqlite /data/monster.sqlite && chown monster:monster /data/monster.sqlite"
   ```

4. Start the server again, and check it:

   ```sh
   sudo docker compose up -d monster
   curl -i https://collection.example.com/health
   ```

## 9. What the server stores about people

For each account, the server stores:

- the email address, and a display name the person can change;
- passkeys: their public key and when they were made and last used (no secrets: the private part of a passkey never leaves the person's device);
- linked TeamSpeak identities: their ID and public key, never a private key;
- the person's collection;
- the app's settings, as the app sends them. The server never looks inside them; which settings these are is decided by the app;
- sessions and sign-in codes, only as hashes (scrambled one-way, so they can't be read back), which run out after 30 days and 10 minutes respectively.

For its rate limits (for example, at most 5 sign-in codes per address per hour), it also keeps short-lived records naming an email address or a network (IP) address, for about an hour.

The server's log lists each request's method, path and result. It doesn't record settings, codes or session tokens. Caddy, as set up here, keeps no access log.

### Deleting someone's data on request

People can delete their own account in Flexspeak, if the app version they use offers it. If someone asks you instead, run this with their email address:

```sh
cd /opt/flexspeak-monster
sudo docker compose exec monster flexspeak-monster delete-account person@example.com
```

It answers `deleted the account of ... and everything in it`, or `there is no account with the address ...`. It removes the account, the collection, the settings, the passkeys, the linked identities and the sessions, the same as when people delete their account themselves. The server keeps running.

Backups taken before still contain the account until they are deleted after 14 days (or whatever `KEEP_DAYS` is). Tell the person so, or delete older backups by hand.

## 10. Troubleshooting

To see what a part is doing, read its log. These show the last 100 lines of each:

```sh
cd /opt/flexspeak-monster
sudo docker compose logs --tail 100 monster
sudo docker compose logs --tail 100 caddy
sudo docker compose logs --tail 100 watchtower
```

### The server doesn't start

Run `sudo docker compose up -d` again and read the message. "Set DOMAIN in .env" (or another name) means that value is missing from `.env`. Otherwise, the server's log (above) says what is wrong on a line starting with `not starting:`.

### The HTTPS certificate isn't issued

Caddy's log mentions `certificate` or `acme` with an error. Check, in this order:

1. The name points at this machine: `nslookup collection.example.com` shows the machine's IP address.
2. Ports 80 and 443 are open to the internet, in the provider's firewall and on the machine. Nothing else on the machine uses them (for example another web server): `sudo ss -tlnp | grep -E ':(80|443) '` should show only Docker.
3. `DOMAIN` in `.env` is exactly the name, without `https://` and without a slash.

After fixing something, restart Caddy with `sudo docker compose restart caddy`. If you tried many times, the certificate authority may make you wait up to an hour before trying again.

### Sign-in emails don't arrive

1. Ask the person to look in their spam folder.
2. In the Resend dashboard, open the list of sent emails (**Emails** or **Logs**). If the email is listed as delivered, the problem is on the receiving side. If it bounced, Resend says why.
3. If it isn't listed at all, look in the server's log for `sign-in code not sent`. Common causes: the API key is wrong or was deleted; the address in `MAIL_FROM` isn't on the verified domain; the domain isn't verified (yet); the daily limit of your Resend plan is used up.
4. After changing `.env`, apply it with `sudo docker compose up -d monster`.

The server sends at most one code per address per minute, and a few per hour, so asking again and again doesn't help.

### Passkeys stop working after the address changed

Every passkey is tied to the server's name (`DOMAIN`). If the name changes, all existing passkeys stop working, and people can't sign in with them. There is no way to move passkeys to a new name. So keep the name. If it has to change anyway, everyone signs in with an emailed code again and creates a new passkey; their accounts and collections are kept.

### "permission denied" or "unable to open database file" in the server's log

The server runs as an unprivileged user (user ID 10001), not as root, and must be able to write to `/data`. The named volume this setup uses is set up correctly by Docker. The problem appears when `/data` is replaced with a folder of the machine that root owns. Either go back to the named volume, or give the folder to that user:

```sh
sudo chown -R 10001:10001 /path/to/the/folder
```

## Hosting on Railway instead

Railway (railway.com) is a hosting service that builds and runs the server for you, so you don't need your own machine, Caddy or Watchtower: Railway provides HTTPS and redeploys on every push to `main` by itself. It costs money depending on use. The menus and options below are as understood at the time of writing; check Railway's current documentation for each step.

1. Create a Railway project and add a service from the GitHub repository `zirkumflex-group/flexspeak` (Railway asks for access to GitHub).
2. In the service's settings, set the Dockerfile path to `server/Dockerfile`. The build must use the repository's root folder, not `server/`, because it needs a shared folder outside it.
3. Add a volume to the service, mounted at `/data`.
4. Set **watch paths** so that only server changes cause a new deploy: `server/**` and `crates/monster-core/**`.
5. Give the service a domain: either Railway's own generated one, or your own name with the DNS record Railway shows. Decide once: as with any host, passkeys are tied to the name.
6. Set the variables:
   - `PUBLIC_URL`: `https://` followed by that domain, such as `https://collection.example.com`;
   - `RESEND_API_KEY` and `MAIL_FROM`, as in section 3;
   - don't set `PORT` yourself. The server listens on the port in `PORT`, or `8787` if there is none. Make sure Railway sends traffic to the port the server listens on; its networking settings show which port it uses (check Railway's documentation).
7. If Railway's volume is owned by root, the server can't write to it. Then set the variable `RAILWAY_RUN_UID=0` to run it as root (check Railway's documentation on volume permissions).
8. Deploy, and check `https://<your domain>/health` as in section 4, step 9.

Railway then deploys every push to `main` that changes those paths. It doesn't use the image GitHub publishes, and `backup.sh` doesn't apply there: use Railway's own volume backups (check whether your plan includes them).
