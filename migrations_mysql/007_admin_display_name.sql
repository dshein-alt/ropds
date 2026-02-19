UPDATE users SET display_name = 'Administrator' WHERE is_superuser = 1 AND display_name = '';
