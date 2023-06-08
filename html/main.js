'use strict';

let router ;
let app ;
let user = {is_logged_in:false};

$(document).ready ( function () {
    Promise.all ( [
            vue_components.loadComponents ( [
                'vue_components/entity.html',
                'vue_components/main_page.html',
                ] ) ,
            new Promise(function(resolve, reject) {
                fetch(new Request("/auth/info"))
                .then((response) => response.json())
                .then((data) => {
                    if (data.user!=null) {
                        data.user.is_logged_in = true;
                        user=data.user;
                    }
                    resolve();
                })
                .catch(reject);
            } ) ,
            // new Promise(function(resolve, reject) {
            //     $.get ( './config.json' , function ( d ) {
            //         wd.api = d.wd.api ;
            //         wd.sparql_url = d.wd.sparql ;
            //         resolve() ;
            //     } , 'json' ) ;
            // } )
    ] ) .then ( () => {

        const routes = [
            { path: '/', component: MainPage , props:true },
            { path: '/:group_id', component: MainPage , props:true },
            // { path: '/tab', component: TablePage , props:true },
            // { path: '/tab/:mode/:main_init', component: TablePage , props:true },
            // { path: '/tab/:mode/:main_init/:cols_init', component: TablePage , props:true },
        ] ;
        router = new VueRouter({routes}) ;
        app = new Vue ( { router } ) .$mount('#app') ;
    } ) ;
} ) ;
