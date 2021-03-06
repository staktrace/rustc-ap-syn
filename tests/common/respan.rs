// Copyright 2018 Syn Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

extern crate syntax;
extern crate syntax_pos;

use self::syntax::ast::{Attribute, Expr, ExprKind, Field, FnDecl, FunctionRetTy, ImplItem,
                        ImplItemKind, Item, ItemKind, Mac, MetaItem, MetaItemKind, MethodSig,
                        NestedMetaItem, NestedMetaItemKind, TraitItem, TraitItemKind, TyParam,
                        Visibility, WhereClause};
use self::syntax::codemap::{self, Spanned};
use self::syntax::fold::{self, Folder};
use self::syntax::parse::token::{Lit, Token};
use self::syntax::ptr::P;
use self::syntax::symbol::Symbol;
use self::syntax::util::move_map::MoveMap;
use self::syntax::util::small_vector::SmallVector;

use self::syntax_pos::{Span, DUMMY_SP};
use self::syntax::ast;

struct Respanner;

impl Respanner {
    fn fold_spanned<T>(&mut self, spanned: Spanned<T>) -> Spanned<T> {
        codemap::respan(self.new_span(spanned.span), spanned.node)
    }

    fn fold_lit(&mut self, l: Lit) -> Lit {
        // Give up on comparing literals inside of macros because there are
        // so many equivalent representations of the same literal; they are
        // tested elsewhere
        match l {
            Lit::Byte(_) => Lit::Byte(Symbol::intern("")),
            Lit::Char(_) => Lit::Char(Symbol::intern("")),
            Lit::Integer(_) => Lit::Integer(Symbol::intern("")),
            Lit::Float(_) => Lit::Float(Symbol::intern("")),
            Lit::Str_(_) => Lit::Str_(Symbol::intern("")),
            Lit::ByteStr(_) => Lit::ByteStr(Symbol::intern("")),
            _ => l,
        }
    }
}

impl Folder for Respanner {
    fn new_span(&mut self, _: Span) -> Span {
        DUMMY_SP
    }

    fn fold_item(&mut self, i: P<Item>) -> SmallVector<P<Item>> {
        let i = i.map(|mut i| {
            i.tokens = None;
            i
        });
        fold::noop_fold_item(i, self)
    }

    fn fold_item_kind(&mut self, i: ItemKind) -> ItemKind {
        match i {
            ItemKind::Fn(decl, unsafety, constness, abi, generics, body) => {
                let generics = self.fold_generics(generics);
                let decl = self.fold_fn_decl(decl);
                let body = self.fold_block(body);
                // default fold_item_kind does not fold this span
                let constness = self.fold_spanned(constness);
                ItemKind::Fn(decl, unsafety, constness, abi, generics, body)
            }
            _ => fold::noop_fold_item_kind(i, self),
        }
    }

    fn fold_expr(&mut self, e: P<Expr>) -> P<Expr> {
        e.map(|e| {
            let folded = fold::noop_fold_expr(e, self);
            Expr {
                node: match folded.node {
                    ExprKind::Lit(l) => {
                        // default fold_expr does not fold lits
                        ExprKind::Lit(l.map(|l| self.fold_spanned(l)))
                    }
                    ExprKind::Binary(op, lhs, rhs) => {
                        // default fold_expr does not fold the op span
                        ExprKind::Binary(
                            self.fold_spanned(op),
                            self.fold_expr(lhs),
                            self.fold_expr(rhs),
                        )
                    }
                    ExprKind::AssignOp(op, lhs, rhs) => {
                        // default fold_expr does not fold the op span
                        ExprKind::AssignOp(
                            self.fold_spanned(op),
                            self.fold_expr(lhs),
                            self.fold_expr(rhs),
                        )
                    }
                    other => other,
                },
                ..folded
            }
        })
    }

    fn fold_ty_param(&mut self, tp: TyParam) -> TyParam {
        TyParam {
            // default fold_ty_param does not fold the span
            span: self.new_span(tp.span),
            ..fold::noop_fold_ty_param(tp, self)
        }
    }

    fn fold_fn_decl(&mut self, decl: P<FnDecl>) -> P<FnDecl> {
        decl.map(
            |FnDecl {
                 inputs,
                 output,
                 variadic,
             }| {
                FnDecl {
                    inputs: inputs.move_map(|x| self.fold_arg(x)),
                    output: match output {
                        FunctionRetTy::Ty(ty) => FunctionRetTy::Ty(self.fold_ty(ty)),
                        // default fold_fn_decl does not fold this span
                        FunctionRetTy::Default(span) => FunctionRetTy::Default(self.new_span(span)),
                    },
                    variadic: variadic,
                }
            },
        )
    }

    fn fold_field(&mut self, field: Field) -> Field {
        Field {
            ident: codemap::respan(
                // default fold_field does not fold this span
                self.new_span(field.ident.span),
                self.fold_ident(field.ident.node),
            ),
            expr: self.fold_expr(field.expr),
            span: self.new_span(field.span),
            is_shorthand: field.is_shorthand,
            attrs: ast::ThinVec::new(),
        }
    }

    fn fold_trait_item(&mut self, mut i: TraitItem) -> SmallVector<TraitItem> {
        i.tokens = None;
        let noop = fold::noop_fold_trait_item(i, self).expect_one("");
        SmallVector::one(TraitItem {
            node: match noop.node {
                TraitItemKind::Method(sig, body) => TraitItemKind::Method(
                    MethodSig {
                        constness: self.fold_spanned(sig.constness),
                        ..sig
                    },
                    body,
                ),
                node => node,
            },
            ..noop
        })
    }

    fn fold_impl_item(&mut self, mut i: ImplItem) -> SmallVector<ImplItem> {
        i.tokens = None;
        let noop = fold::noop_fold_impl_item(i, self).expect_one("");
        SmallVector::one(ImplItem {
            node: match noop.node {
                ImplItemKind::Method(sig, body) => ImplItemKind::Method(
                    MethodSig {
                        constness: self.fold_spanned(sig.constness),
                        ..sig
                    },
                    body,
                ),
                node => node,
            },
            ..noop
        })
    }

    fn fold_attribute(&mut self, mut at: Attribute) -> Option<Attribute> {
        at.id.0 = 0;
        fold::noop_fold_attribute(at, self)
    }

    fn fold_meta_item(&mut self, meta_item: MetaItem) -> MetaItem {
        let MetaItem { name, node, span } = meta_item;
        MetaItem {
            name: name,
            node: match node {
                MetaItemKind::Word => MetaItemKind::Word,
                MetaItemKind::List(nested) => {
                    MetaItemKind::List(nested.move_map(|e| self.fold_meta_list_item(e)))
                }
                // default fold_meta_item does not fold the value span
                MetaItemKind::NameValue(lit) => MetaItemKind::NameValue(self.fold_spanned(lit)),
            },
            span: self.new_span(span),
        }
    }

    fn fold_meta_list_item(&mut self, list_item: NestedMetaItem) -> NestedMetaItem {
        Spanned {
            node: match list_item.node {
                NestedMetaItemKind::MetaItem(mi) => {
                    NestedMetaItemKind::MetaItem(self.fold_meta_item(mi))
                }
                // default fold_meta_list_item does not fold the span
                NestedMetaItemKind::Literal(lit) => {
                    NestedMetaItemKind::Literal(self.fold_spanned(lit))
                }
            },
            span: self.new_span(list_item.span),
        }
    }

    // This folder is disabled by default.
    fn fold_mac(&mut self, mac: Mac) -> Mac {
        fold::noop_fold_mac(mac, self)
    }

    fn fold_token(&mut self, t: Token) -> Token {
        fold::noop_fold_token(
            match t {
                // default fold_token does not fold literals
                Token::Literal(lit, repr) => Token::Literal(self.fold_lit(lit), repr),
                _ => t,
            },
            self,
        )
    }

    fn fold_vis(&mut self, vis: Visibility) -> Visibility {
        fold::noop_fold_vis(
            match vis {
                Visibility::Crate(span, sugar) => Visibility::Crate(self.new_span(span), sugar),
                _ => vis,
            },
            self,
        )
    }

    // noop_fold_where_clause doesn't modify the span.
    fn fold_where_clause(&mut self, mut clause: WhereClause) -> WhereClause {
        clause.span = self.new_span(clause.span);
        fold::noop_fold_where_clause(clause, self)
    }
}

#[allow(dead_code)]
pub fn respan_crate(krate: ast::Crate) -> ast::Crate {
    Respanner.fold_crate(krate)
}

#[allow(dead_code)]
pub fn respan_expr(expr: P<ast::Expr>) -> P<ast::Expr> {
    Respanner.fold_expr(expr)
}
