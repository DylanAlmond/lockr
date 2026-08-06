import { createRouter, createWebHistory, RouteRecordRaw } from 'vue-router';
import AppLayout from '../components/layout/AppLayout.vue';
import AuthView from '../components/views/AuthView.vue';
import { useUser } from '../composables/useUser';
import AccountList from '../components/panel/AccountList.vue';
import AccountPanel from '../components/panel/AccountPanel.vue';

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    component: AppLayout,
    meta: { requiresAuth: true },
    redirect: '/all-items',
    children: [
      {
        path: 'all-items/:accountId?',
        name: 'all-items',
        meta: { requiresAuth: true },
        components: {
          list: AccountList,
          panel: AccountPanel
        },
        props: {
          list: {
            favourite_only: false,
            recently_accessed: false,
            vault_id: null
          }
        }
      },
      {
        path: 'favourites/:accountId?',
        name: 'favourites',
        meta: { requiresAuth: true },
        components: {
          list: AccountList,
          panel: AccountPanel
        },
        props: {
          list: {
            favourite_only: true,
            recently_accessed: false,
            vault_id: null
          }
        }
      },
      {
        path: 'recently-accessed/:accountId?',
        name: 'recently-accessed',
        meta: { requiresAuth: true },
        components: {
          list: AccountList,
          panel: AccountPanel
        },
        props: {
          list: {
            favourite_only: false,
            recently_accessed: true,
            vault_id: null
          }
        }
      },
      {
        name: 'vault',
        path: 'vault/:vaultId/:accountId?',
        meta: { requiresAuth: true },
        components: {
          list: AccountList,
          panel: AccountPanel
        },
        props: {
          list: (route) => ({
            favourite_only: false,
            recently_accessed: false,
            vault_id: route.params.vaultId
          })
        }
      }
    ]
  },
  {
    path: '/auth',
    component: AuthView
  },

  /* 404 fallback */
  {
    path: '/:pathMatch(.*)*',
    redirect: '/all-items'
  }
];

const router = createRouter({
  history: createWebHistory(),
  routes
});

router.beforeEach((to, _from) => {
  const { user } = useUser();

  if (to.meta.requiresAuth && !user.value) {
    return { path: '/auth', replace: true };
  }

  if (to.path === '/auth' && user.value) {
    return { path: '/all-items', replace: true };
  }
});

export default router;
