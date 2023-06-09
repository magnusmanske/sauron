'use strict';

let router ;
let app ;
let user = {is_logged_in:false};
let gobal_config = {};

$(document).ready ( function () {
    Promise.all ( [
            vue_components.loadComponents ( [
                'vue_components/entity.html',
                'vue_components/user.html',
                'vue_components/main_page.html',
                'vue_components/entity_page.html',
                'vue_components/access_page.html',
                'vue_components/search_dropdown.html',
                ] ) ,
            new Promise(function(resolve, reject) {
                fetch(new Request("/auth/info"))
                .then((response) => response.json())
                .then((data) => {
                    if (data.user!=null) {
                        data.user.is_logged_in = true;
                        user=data.user;
                        $("#login").text("Logged in as "+user.name);
                        $('#logout').show();
                    }
                    resolve();
                })
                .catch(reject);
            } ) ,
    ] ) .then ( () => {

        const routes = [
            { path: '/', component: MainPage , props:true },
            { path: '/:group_id', component: MainPage , props:true },
            { path: '/entity/:entity_id', component: EntityPage , props:true },
            { path: '/access/:entity_id/:user_id/:rights', component: AccessPage , props:true },
        ] ;
        router = new VueRouter({routes}) ;
        app = new Vue ( { router } ) .$mount('#app') ;
    } ) ;
} ) ;
