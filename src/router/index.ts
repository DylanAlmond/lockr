import { createRouter, createWebHistory, RouteRecordRaw } from 'vue-router';
import AppLayout from '../components/layout/AppLayout.vue';
import AllItemsView from '../components/views/AllItemsView.vue';
import EmptyPanel from '../components/panel/EmptyPanel.vue';
import PasswordForm from '../components/panel/PasswordForm.vue';
import PasswordDetail from '../components/panel/PasswordDetail.vue';
import FavouritesView from '../components/views/FavouritesView.vue';
import RecentlyAccessedView from '../components/views/RecentlyAccessedView.vue';
import VaultsView from '../components/views/VaultsView.vue';
import VaultOverview from '../components/panel/VaultOverview.vue';
import VaultPasswordsVue from '../components/views/VaultPasswordsVue.vue';
import VaultForm from '../components/panel/VaultForm.vue';

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    component: AppLayout,
    redirect: '/all-items',
    children: [
      {
        path: 'all-items',
        components: {
          list: AllItemsView,
          panel: EmptyPanel
        },
        children: [
          {
            path: 'new',
            components: { panel: PasswordForm }
          },
          {
            path: ':id',
            components: { panel: PasswordDetail }
          },
          {
            path: ':id/edit',
            components: { panel: PasswordForm }
          }
        ]
      },
      {
        path: 'favourites',
        components: {
          list: FavouritesView,
          panel: EmptyPanel
        },
        children: [
          {
            path: ':id',
            components: { panel: PasswordDetail }
          },
          {
            path: ':id/edit',
            components: { panel: PasswordForm }
          }
        ]
      },
      {
        path: 'recently-accessed',
        components: {
          list: RecentlyAccessedView,
          panel: EmptyPanel
        },
        children: [
          {
            path: ':id',
            components: { panel: PasswordDetail }
          },
          {
            path: ':id/edit',
            components: { panel: PasswordForm }
          }
        ]
      },
      {
        path: 'vaults',
        components: {
          list: VaultsView,
          panel: VaultOverview
        },
        children: [
          {
            path: 'new',
            components: { panel: VaultForm }
          },
          {
            path: 'edit/:id',
            components: { panel: VaultForm }
          },
          {
            path: ':vaultId',
            components: {
              list: VaultPasswordsVue,
              panel: EmptyPanel
            },
            children: [
              {
                path: 'new',
                components: { panel: PasswordForm }
              },
              {
                path: ':id',
                components: { panel: PasswordDetail }
              },
              {
                path: ':id/edit',
                components: { panel: PasswordForm }
              }
            ]
          }
        ]
      }
    ]
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

export default router;
