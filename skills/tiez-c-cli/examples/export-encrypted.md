# Example: Encrypted backup workflow

## Scenario

You need a portable, encrypted backup of your clipboard history before
reinstalling your operating system. The workflow exports the database to a
single `.tiez` file, verifies the backup, and finally restores it to the live
database only after the restore has been validated against a temporary copy.

## Prerequisites

- A passphrase of at least 12 characters. Treat this passphrase like a password:
  store it in a password manager, not in a shell history file.
- Enough disk space for two copies of the database (export + test restore).
- `tiez-c` version that supports `export --encrypted` and `import --passphrase`.

## Steps

1. Export the current database to an encrypted file with a timestamped name.

```bash
tiez-c export ~/backups/tiez-$(date +%Y%m%d).tiez --encrypted \
  --passphrase "your-strong-passphrase-here"
```

2. Verify that the file is non-trivial in size (an empty export is a bad sign).

```bash
ls -la ~/backups/tiez-*.tiez
```

3. Test the restore against a separate temporary database to confirm the
   passphrase and file integrity.

```bash
TIEZ_DB_PATH=/tmp/test-restore.db tiez-c import \
  ~/backups/tiez-20260619.tiez \
  --passphrase "your-strong-passphrase-here"
```

4. Confirm that the temporary database contains the expected number of entries.

```bash
TIEZ_DB_PATH=/tmp/test-restore.db tiez-c stats
```

5. If the test restore looks correct, restore to the live database. Use
   `--mode replace` only after you are confident the backup is complete.

```bash
tiez-c import ~/backups/tiez-20260619.tiez \
  --mode replace \
  --passphrase "your-strong-passphrase-here"
```

6. Final verification against the live database.

```bash
tiez-c stats
```

## Expected output

- Step 1 prints a success message and the path to the encrypted file.
- Step 2 shows a file size in the megabyte range for a typical personal history.
- Step 3 completes without error; the terminal returns to the prompt.
- Step 4 prints entry counts that match or exceed the pre-export `tiez-c stats`
  output from the live database.
- Step 5 prints a confirmation that the live database was replaced.
- Step 6 shows the same or higher entry count, confirming the restore succeeded.

## Recovery procedure

If the live restore fails or the wrong passphrase was used, re-run step 3 with
the same encrypted file against a fresh temp database to confirm the file is
still valid. Once confirmed, repeat step 5 with extra care around quoting the
passphrase.

## Security notes

- Never store the passphrase in a script file or shell history. Use `read -s`
  inside a subshell if you need to avoid typing it in plain text:

```bash
read -s -p "Passphrase: " PASSPHRASE
tiez-c export ~/backups/backup.tiez --encrypted --passphrase "$PASSPHRASE"
```

- Restrict permissions on the backup file: `chmod 600 ~/backups/tiez-*.tiez`.
- Delete the temporary test database after verification:

```bash
rm /tmp/test-restore.db
```
