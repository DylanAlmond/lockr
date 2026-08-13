import { invoke } from '@tauri-apps/api/core';
import type { User, Vault, Account } from './types';

const MASTER_PASSWORD = 'test12';

function generatePassword(length: number = 16): string {
  const chars =
    'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()-_=+[]{}<>?';
  let password = '';

  for (let i = 0; i < length; i++) {
    const randomIndex = Math.floor(Math.random() * chars.length);
    password += chars[randomIndex];
  }

  return password;
}

async function createVault(name: string): Promise<Vault | null> {
  return await invoke<Vault>('create_vault', { name });
}

async function getUnlockedVaults(): Promise<Vault[]> {
  return await invoke<Vault[]>('get_unlocked_vaults');
}

async function addAccount(
  vaultId: string,
  username: string,
  password: string,
  displayName?: string | null,
  email?: string | null
): Promise<Account | null> {
  return await invoke<Account>('add_account', {
    vaultId,
    displayName: displayName || null,
    username,
    email: email || null,
    password
  });
}

async function addUser(
  name: string,
  color: string,
  icon: string,
  masterPassword: string
): Promise<User | null> {
  await invoke<Vault[]>('register_user', { name, masterPassword });

  await invoke('update_profile', {
    color: color,
    icon: icon
  });

  return await invoke<User>('get_user');
}

async function run() {
  console.log(`Master Password: ${MASTER_PASSWORD}`);

  try {
    await addUser('John Doe', '#ff0000', 'icon', MASTER_PASSWORD);

    await createVault('Work');
    await createVault('Family');

    const vaults = await getUnlockedVaults();

    const vaultA = vaults[0];
    const vaultB = vaults[1];
    const vaultC = vaults[2];

    console.log(`Logged in. Vault IDs: [${vaults.map((v) => v.id).join(', ')}]`);

    const test_accounts: {
      vaultId: string;
      username: string;
      password: string;
      displayName?: string | null;
      email?: string | null;
    }[] = [
      // ==========================
      // Vault A
      // ==========================
      {
        vaultId: vaultA.id,
        username: 'admin@example.com',
        password: generatePassword(),
        displayName: 'Adobe',
        email: 'admin@example.com'
      },
      {
        vaultId: vaultA.id,
        username: 'john.doe@gmail.com',
        password: generatePassword(),
        displayName: 'Google',
        email: 'john.doe@gmail.com'
      },
      {
        vaultId: vaultA.id,
        username: 'john.doe',
        password: generatePassword(),
        displayName: 'GitHub'
      },
      {
        vaultId: vaultA.id,
        username: 'john.doe',
        password: generatePassword(),
        displayName: 'GitLab'
      },
      {
        vaultId: vaultA.id,
        username: 'johndoe89',
        password: generatePassword(),
        displayName: 'Reddit'
      },
      {
        vaultId: vaultA.id,
        username: 'john.doe@outlook.com',
        password: generatePassword(),
        displayName: 'Microsoft',
        email: 'john.doe@outlook.com'
      },
      {
        vaultId: vaultA.id,
        username: 'jdoe',
        password: generatePassword(),
        displayName: 'Slack',
        email: 'john.doe@company.com'
      },
      {
        vaultId: vaultA.id,
        username: 'john.doe@company.com',
        password: generatePassword(),
        displayName: 'Notion',
        email: 'john.doe@company.com'
      },
      {
        vaultId: vaultA.id,
        username: 'john.doe@icloud.com',
        password: generatePassword(),
        displayName: 'Apple ID',
        email: 'john.doe@icloud.com'
      },
      {
        vaultId: vaultA.id,
        username: 'john_doe',
        password: generatePassword(),
        displayName: 'Discord'
      },

      // ==========================
      // Vault B
      // ==========================
      {
        vaultId: vaultB.id,
        username: 'sarah.williams@gmail.com',
        password: generatePassword(),
        displayName: 'Amazon',
        email: 'sarah.williams@gmail.com'
      },
      {
        vaultId: vaultB.id,
        username: 'sarah.williams@gmail.com',
        password: generatePassword(),
        displayName: 'Netflix',
        email: 'sarah.williams@gmail.com'
      },
      {
        vaultId: vaultB.id,
        username: 'sarah.williams',
        password: generatePassword(),
        displayName: 'Spotify'
      },
      {
        vaultId: vaultB.id,
        username: 'sarahw',
        password: generatePassword(),
        displayName: 'Dropbox'
      },
      {
        vaultId: vaultB.id,
        username: 'sarah@outlook.com',
        password: generatePassword(),
        displayName: 'PayPal',
        email: 'sarah@outlook.com'
      },
      {
        vaultId: vaultB.id,
        username: 'sarahwilliams',
        password: generatePassword(),
        displayName: 'X'
      },
      {
        vaultId: vaultB.id,
        username: 'sarah.williams',
        password: generatePassword(),
        displayName: 'LinkedIn',
        email: 'sarah.williams@gmail.com'
      },
      {
        vaultId: vaultB.id,
        username: 'sarah.williams',
        password: generatePassword(),
        displayName: 'Zoom',
        email: 'sarah.williams@gmail.com'
      },
      {
        vaultId: vaultB.id,
        username: 'sarah@company.com',
        password: generatePassword(),
        displayName: 'Jira',
        email: 'sarah@company.com'
      },
      {
        vaultId: vaultB.id,
        username: 'sarah@company.com',
        password: generatePassword(),
        displayName: 'Confluence',
        email: 'sarah@company.com'
      },

      // ==========================
      // Vault C
      // ==========================
      {
        vaultId: vaultC.id,
        username: 'michael.brown@yahoo.com',
        password: generatePassword(),
        displayName: 'Steam',
        email: 'michael.brown@yahoo.com'
      },
      {
        vaultId: vaultC.id,
        username: 'mikebrown',
        password: generatePassword(),
        displayName: 'Epic Games'
      },
      {
        vaultId: vaultC.id,
        username: 'michael.brown',
        password: generatePassword(),
        displayName: 'EA'
      },
      {
        vaultId: vaultC.id,
        username: 'michael.brown',
        password: generatePassword(),
        displayName: 'Ubisoft'
      },
      {
        vaultId: vaultC.id,
        username: 'mbrown',
        password: generatePassword(),
        displayName: 'DigitalOcean'
      },
      {
        vaultId: vaultC.id,
        username: 'michael@proton.me',
        password: generatePassword(),
        displayName: 'Proton',
        email: 'michael@proton.me'
      },
      {
        vaultId: vaultC.id,
        username: 'michael.brown@gmail.com',
        password: generatePassword(),
        displayName: 'YouTube',
        email: 'michael.brown@gmail.com'
      },
      {
        vaultId: vaultC.id,
        username: 'michael.brown@gmail.com',
        password: generatePassword(),
        displayName: 'Twitch',
        email: 'michael.brown@gmail.com'
      },
      {
        vaultId: vaultC.id,
        username: 'mbrown92',
        password: generatePassword(),
        displayName: 'Facebook'
      },
      {
        vaultId: vaultC.id,
        username: 'michael.brown',
        password: generatePassword(),
        displayName: 'Instagram'
      }
    ];

    for (const acc of test_accounts) {
      console.log(`Creating account: ${acc.username}`);

      const account = (await addAccount(
        acc.vaultId,
        acc.username,
        acc.password,
        acc.displayName,
        acc.email
      )) as Account;

      console.log(`✅ Created account: ${account.username}`);
    }

    console.log('\n✅ Dev seed data generated successfully!');
    console.log('You can now log in with password:', MASTER_PASSWORD);
    console.log(
      '\n⚠️ WARNING: This script uses TS for speed. Keep secrets strictly in Rust for production.'
    );
  } catch (e) {
    console.error('❌ Seed failed:', e);
  }
}

export default run;
