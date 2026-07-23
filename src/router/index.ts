import { createRouter, createWebHistory, RouteRecordRaw } from 'vue-router';
import AppLayout from '../components/layout/AppLayout.vue';
import PasswordDetail from '../components/panel/PasswordDetail.vue';
import AuthView from '../components/views/AuthView.vue';
import { useUser } from '../composables/useUser';
import AllItemsView from '../components/views/AllItemsView.vue';
import FavouritesView from '../components/views/FavouritesView.vue';
import RecentlyAccessedView from '../components/views/RecentlyAccessedView.vue';
import VaultView from '../components/views/VaultView.vue';

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    component: AppLayout,
    meta: { requiresAuth: true },
    redirect: '/all-items',
    children: [
      {
        path: 'all-items',
        meta: { requiresAuth: true },
        components: {
          list: AllItemsView,
          panel: PasswordDetail
        }
      },
      {
        path: 'favourites',
        meta: { requiresAuth: true, favourites: true },
        components: {
          list: FavouritesView,
          panel: PasswordDetail
        }
      },
      {
        path: 'recently-accessed',
        meta: { requiresAuth: true, recentlyAccessed: true },
        components: {
          list: RecentlyAccessedView,
          panel: PasswordDetail
        }
      },
      {
        path: 'vault/:vaultId',
        meta: { requiresAuth: true },
        components: {
          list: VaultView,
          panel: PasswordDetail
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
