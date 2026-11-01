INSERT INTO settings (key, value, updated_at)
SELECT 'skip_security_confirmations', 'false', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE EXISTS (SELECT 1 FROM settings)
ON CONFLICT(key) DO NOTHING;
