/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { AddTeamMembersInput } from '../models/AddTeamMembersInput';
import type { CreateGameInput } from '../models/CreateGameInput';
import type { CreateTeamInput } from '../models/CreateTeamInput';
import type { CreateUserInput } from '../models/CreateUserInput';
import type { Game } from '../models/Game';
import type { GameWithInvitations } from '../models/GameWithInvitations';
import type { RespondToInvitationInput } from '../models/RespondToInvitationInput';
import type { Team } from '../models/Team';
import type { TeamListItem } from '../models/TeamListItem';
import type { User } from '../models/User';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class DefaultService {
    /**
     * @returns string
     * @throws ApiError
     */
    public static getPing(): CancelablePromise<string> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/ping',
        });
    }
    /**
     * @returns User
     * @throws ApiError
     */
    public static getUsersMe(): CancelablePromise<User> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/users/me',
        });
    }
    /**
     * @param requestBody
     * @returns User
     * @throws ApiError
     */
    public static postUsers(
        requestBody: CreateUserInput,
    ): CancelablePromise<User> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/users',
            body: requestBody,
            mediaType: 'application/json; charset=utf-8',
        });
    }
    /**
     * @param q
     * @returns User
     * @throws ApiError
     */
    public static getUsersSearch(
        q: string,
    ): CancelablePromise<Array<User>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/users/search',
            query: {
                'q': q,
            },
        });
    }
    /**
     * @param requestBody
     * @returns Team
     * @throws ApiError
     */
    public static postTeams(
        requestBody: CreateTeamInput,
    ): CancelablePromise<Team> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/teams',
            body: requestBody,
            mediaType: 'application/json; charset=utf-8',
        });
    }
    /**
     * @returns TeamListItem
     * @throws ApiError
     */
    public static getTeams(): CancelablePromise<Array<TeamListItem>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/teams',
        });
    }
    /**
     * @param id
     * @returns Team
     * @throws ApiError
     */
    public static getTeams1(
        id: string,
    ): CancelablePromise<Team> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/teams/{id}',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @param teamId
     * @param requestBody
     * @returns any
     * @throws ApiError
     */
    public static postTeamsMembers(
        teamId: string,
        requestBody: AddTeamMembersInput,
    ): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/teams/{team_id}/members',
            path: {
                'team_id': teamId,
            },
            body: requestBody,
            mediaType: 'application/json; charset=utf-8',
        });
    }
    /**
     * @param requestBody
     * @returns Game
     * @throws ApiError
     */
    public static postGames(
        requestBody: CreateGameInput,
    ): CancelablePromise<Game> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/games',
            body: requestBody,
            mediaType: 'application/json; charset=utf-8',
        });
    }
    /**
     * @returns Game
     * @throws ApiError
     */
    public static getGames(): CancelablePromise<Array<Game>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/games',
        });
    }
    /**
     * @param id
     * @returns GameWithInvitations
     * @throws ApiError
     */
    public static getGames1(
        id: string,
    ): CancelablePromise<GameWithInvitations> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/games/{id}',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @param gameId
     * @param requestBody
     * @returns any
     * @throws ApiError
     */
    public static postGamesInvitations(
        gameId: string,
        requestBody: AddTeamMembersInput,
    ): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/games/{game_id}/invitations',
            path: {
                'game_id': gameId,
            },
            body: requestBody,
            mediaType: 'application/json; charset=utf-8',
        });
    }
    /**
     * @param gameId
     * @param userId
     * @param requestBody
     * @returns any
     * @throws ApiError
     */
    public static putGamesInvitations(
        gameId: string,
        userId: string,
        requestBody: RespondToInvitationInput,
    ): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/games/{game_id}/invitations/{user_id}',
            path: {
                'game_id': gameId,
                'user_id': userId,
            },
            body: requestBody,
            mediaType: 'application/json; charset=utf-8',
        });
    }
}
