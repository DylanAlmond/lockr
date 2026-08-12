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
        // :accountId? handles viewing an account
        // :mode? handles 'edit' or 'create'
        path: 'all-items/:accountId?/:mode?',
        name: 'all-items',
        meta: { requiresAuth: true },
        components: {
          list: AccountList,
          panel: AccountPanel
        },
        props: {
          // Pass dynamic filters based on the URL query params
          list: (route) => ({
            favourite_only: route.query.filter === 'favourites',
            recently_accessed: route.query.filter === 'recently-accessed',
            vault_id: null,
            tags: route.query.tags || null
          })
        }
      },
      {
        path: 'vault/:vaultId/:accountId?/:mode?',
        name: 'vault',
        meta: { requiresAuth: true },
        components: {
          list: AccountList,
          panel: AccountPanel
        },
        props: {
          list: (route) => ({
            favourite_only: false,
            recently_accessed: false,
            vault_id: route.params.vaultId,
            tags: route.query.tags || null
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
