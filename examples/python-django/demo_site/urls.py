from django.urls import path

from . import views

urlpatterns = [
    path("", views.middleware_mode, name="middleware"),
    path("helper", views.helper_mode, name="helper"),
    path("health", views.health, name="health"),
]
