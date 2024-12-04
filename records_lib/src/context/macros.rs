macro_rules! impl_ctx {
    () => {
        #[inline(always)]
        fn get_opt_event(&self) -> Option<&models::Event> {
            <E as Ctx>::get_opt_event(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_event_id(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition(&self) -> Option<&models::EventEdition> {
            <E as Ctx>::get_opt_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_edition_id(&self) -> Option<u32> {
            <E as Ctx>::get_opt_edition_id(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_edition(&self) -> Option<(&models::Event, &models::EventEdition)> {
            <E as Ctx>::get_opt_event_edition(&self.extra)
        }

        #[inline(always)]
        fn get_opt_event_edition_ids(&self) -> Option<(u32, u32)> {
            <E as Ctx>::get_opt_event_edition_ids(&self.extra)
        }
    };

    ($($t:tt)*) => { $($t)* };
}

pub(super) use impl_ctx;

macro_rules! new_combinator {
    (
        'combinator: struct $Combinator:ident $(<$lt:lifetime>)? {$(
            $field:ident: $ty:ty
        ),* $(,)?}

        $( 'trait $(needs [$($subtrait:ident),* $(,)?])?: $($trait_lt:lifetime)? trait $AssociatedTrait:ident.$trait_fn:ident -> $trait_fn_ty:ty |$trait_self:ident| {
            $expr:expr
        } )?

        'delegates {$(
            $($delegate_trait_lt:lifetime)? $DelegateTrait:ident.$fn:ident -> $fn_ty:ty
        ),* $(,)?}

        $( 'ctx_impl {$($ctx_impl_tt:tt)*} )?

        $( 'addon_impls {$(
            $($addon_trait_lt:lifetime)? $AddonTrait:ident.$addon_fn:ident -> $addon_fn_ty:ty |$addon_self:ident| {
                $addon_expr:expr
            }
        ),* $(,)?} )?
    ) => {
        pub struct $Combinator <$($lt,)? E> {
            $( $field: $ty, )*
            extra: E,
        }

        impl <$($lt,)? E: Ctx> Ctx for $Combinator <$($lt,)? E> {
            $crate::context::macros::impl_ctx!($($($ctx_impl_tt)*)?);
        }

        $(
            pub trait $AssociatedTrait: Ctx $( $(+ $subtrait)* )? {
                fn $trait_fn(&self) -> $trait_fn_ty;
            }

            impl<T: $AssociatedTrait> $AssociatedTrait for &T {
                #[inline]
                fn $trait_fn(&self) -> $trait_fn_ty {
                    <T as $AssociatedTrait>::$trait_fn(self)
                }
            }

            impl<T: $AssociatedTrait> $AssociatedTrait for &mut T {
                #[inline]
                fn $trait_fn(&self) -> $trait_fn_ty {
                    <T as $AssociatedTrait>::$trait_fn(self)
                }
            }

            impl <$($trait_lt,)? E: Ctx> $AssociatedTrait for $Combinator <$($trait_lt,)? E> {
                fn $trait_fn($trait_self: &Self) -> $trait_fn_ty {
                    $expr
                }
            }
        )?

        $( $(
            impl <$($addon_trait_lt,)? E: Ctx> $AddonTrait for $Combinator <$($addon_trait_lt,)? E> {
                fn $addon_fn($addon_self: &Self) -> $addon_fn_ty {
                    $addon_expr
                }
            }
        )* )?

        $(
            impl <$($delegate_trait_lt,)? E: $DelegateTrait> $DelegateTrait for $Combinator <$($delegate_trait_lt,)? E> {
                #[inline]
                fn $fn(&self) -> $fn_ty {
                    <E as $DelegateTrait>::$fn(&self.extra)
                }
            }
        )*
    };
}

pub(super) use new_combinator;
